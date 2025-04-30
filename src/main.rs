mod cli;
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::network::Network;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::ServiceFlags;
use clap::Parser;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
struct ServiceFlagsWrapper(ServiceFlags);

impl Serialize for ServiceFlagsWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.0.to_u64())
    }
}

impl<'de> Deserialize<'de> for ServiceFlagsWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bits = u64::deserialize(deserializer)?;
        Ok(ServiceFlagsWrapper(ServiceFlags::from(bits)))
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct PeerInfo {
    address: SocketAddr,
    last_seen: u64,
    last_connected: Option<u64>,
    status: PeerStatus,
    service_flags: Option<ServiceFlagsWrapper>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum PeerStatus {
    NeverTried,
    ConnectedRecently,
    Unreachable,
    Banned,
}

impl PeerStatus {
    fn from_string(status: &str) -> Self {
        match status {
            "NeverTried" => PeerStatus::NeverTried,
            "ConnectedRecently" => PeerStatus::ConnectedRecently,
            "Unreachable" => PeerStatus::Unreachable,
            "Banned" => PeerStatus::Banned,
            _ => PeerStatus::NeverTried,
        }
    }
}

impl ToString for PeerStatus {
    fn to_string(&self) -> String {
        match self {
            PeerStatus::NeverTried => "NeverTried".to_string(),
            PeerStatus::ConnectedRecently => "ConnectedRecently".to_string(),
            PeerStatus::Unreachable => "Unreachable".to_string(),
            PeerStatus::Banned => "Banned".to_string(),
        }
    }
}

impl PeerInfo {
    fn new(address: SocketAddr) -> Self {
        Self {
            address,
            last_seen: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            last_connected: None,
            status: PeerStatus::NeverTried,
            service_flags: None,
        }
    }
}

fn initialize_database(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS peers (
            address TEXT PRIMARY KEY,
            last_seen INTEGER NOT NULL,
            last_connected INTEGER,
            status TEXT NOT NULL,
            service_flags INTEGER
        )",
        [],
    )?;
    Ok(conn)
}

fn save_peers_to_database(conn: &Connection, peers: &HashMap<SocketAddr, PeerInfo>) -> Result<()> {
    for (address, peer_info) in peers {
        conn.execute(
            "INSERT OR REPLACE INTO peers (address, last_seen, last_connected, status, service_flags)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                address.to_string(),
                peer_info.last_seen,
                peer_info.last_connected,
                peer_info.status.to_string(),
                peer_info.service_flags.map(|flags| flags.0.to_u64()).unwrap_or(0),
            ],
        )?;
    }
    Ok(())
}

fn load_peers_from_database(conn: &Connection) -> Result<HashMap<SocketAddr, PeerInfo>> {
    let mut stmt = conn.prepare("SELECT address, last_seen, last_connected, status FROM peers")?;
    let peer_iter = stmt.query_map([], |row| {
        let address: String = row.get(0)?;
        let socket_addr: SocketAddr = address.parse().unwrap();
        Ok((
            socket_addr,
            PeerInfo {
                address: socket_addr,
                last_seen: row.get(1)?,
                last_connected: row.get(2).ok(),
                status: PeerStatus::from_string(row.get::<_, String>(3)?.as_str()),
                service_flags: None,
            },
        ))
    })?;

    let mut peers = HashMap::new();
    for peer in peer_iter {
        let (addr, info) = peer?;
        peers.insert(addr, info);
    }
    Ok(peers)
}

fn reconnect_using_database(peers: &mut HashMap<SocketAddr, PeerInfo>, conn: &Connection) {
    // Load peers from database
    let loaded_peers = load_peers_from_database(conn).unwrap_or_else(|_| HashMap::new());

    // Merge loaded peers into the in-memory peers
    for (addr, peer_info) in loaded_peers {
        peers.entry(addr).or_insert(peer_info);
    }

    // Attempt to connect to a few peers from the list
    for (addr, peer_info) in peers.iter_mut() {
        if peer_info.status == PeerStatus::Banned {
            continue;
        }

        let timeout_duration = std::time::Duration::from_secs(5);
        match TcpStream::connect_timeout(addr, timeout_duration) {
            Ok(_) => {
                println!("[+] Reconnected to peer: {}", addr);
                peer_info.last_connected = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
                peer_info.status = PeerStatus::ConnectedRecently;
            }
            Err(_) => {
                println!("[-] Failed to reconnect to peer: {}", addr);
                peer_info.status = PeerStatus::Unreachable;
            }
        }
    }

    // Save updated peers back to database
    save_peers_to_database(conn, peers).expect("Failed to save peers to database");
}

fn perform_handshake(stream: &mut TcpStream, network: Network) -> Option<ServiceFlags> {
    // Send version message
    let version_message = VersionMessage {
        version: 70015,
        services: ServiceFlags::NETWORK, // Advertise our supported features
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        receiver: Address::new(
            &stream.peer_addr().unwrap(),
            ServiceFlags::NETWORK,
        ),
        sender: Address::new(
            &stream.local_addr().unwrap(),
            ServiceFlags::NETWORK,
        ),
        nonce: rand::random(),
        user_agent: "/rust-seminar:0.1.0/".to_string(),
        start_height: 0,
        relay: false,
    };

    let version_network_message =
        RawNetworkMessage::new(network.magic(), NetworkMessage::Version(version_message));

    if let Err(e) = stream.write_all(&serialize(&version_network_message)) {
        println!("[-] Failed to send version message: {}", e);
        return None;
    }

    println!("[+] Sent version message");

    // Read response
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut buffer = vec![0; 1024];

    if let Ok(bytes_read) = reader.read(&mut buffer) {
        if let Ok(response) = deserialize::<RawNetworkMessage>(&buffer[..bytes_read]) {
            if let NetworkMessage::Version(version) = response.payload() {
                println!("[+] Received version message: {:?}", version);

                // Send verack message
                let verack_message = RawNetworkMessage::new(network.magic(), NetworkMessage::Verack);
                if let Err(e) = stream.write_all(&serialize(&verack_message)) {
                    println!("[-] Failed to send verack message: {}", e);
                    return None;
                }

                println!("[+] Sent verack message");

                // Return the service flags from the version message
                return Some(version.services);
            }
        }
    }

    println!("[-] Failed to complete handshake");
    None
}

fn main() {
    let args = cli::Cli::parse();

    let mut peers: HashMap<SocketAddr, PeerInfo> = HashMap::new();
    let db_path = "peers.db";

    // Initialize SQLite database
    let conn = initialize_database(db_path).expect("Failed to initialize database");

    // Reconnect using the database
    reconnect_using_database(&mut peers, &conn);

    let target_addr = match (args.host.as_str(), args.port).to_socket_addrs() {
        Ok(mut addrs) => addrs.next().unwrap_or_else(|| {
            eprintln!("[-] Failed to resolve address for {}:{}", args.host, args.port);
            std::process::exit(1);
        }),
        Err(e) => {
            eprintln!("[-] Error resolving address for {}:{} - {}", args.host, args.port, e);
            std::process::exit(1);
        }
    };

    let timeout_duration = std::time::Duration::from_secs(args.timeout);

    let stream = TcpStream::connect_timeout(&target_addr, timeout_duration);
    let mut stream = match stream {
        Ok(tcpstream) => {
            println!("[+] Connected to {}", target_addr);
            tcpstream
        }
        Err(e) => {
            println!("[-] Error connecting to {}: {}", target_addr, e);
            return;
        }
    };

    println!("[+] CLI Arguments: {:?}", args);

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    println!("[+] Reader created");

    let network = Network::Bitcoin;
    println!("[+] Magic bytes: {:?}", network.magic());

    let service_flags = perform_handshake(&mut stream, network);
    if let Some(flags) = service_flags {
        println!("[+] Node supports service flags: {:?}", flags);
        if let Some(peer_info) = peers.get_mut(&target_addr) {
            peer_info.status = PeerStatus::ConnectedRecently;
            peer_info.last_connected = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
            // Save service flags to the database
            save_peers_to_database(&conn, &peers).expect("Failed to save peers to database");
        }
    } else {
        println!("[-] Handshake failed or no service flags received");
    }

    let version_message = VersionMessage {
        version: 70015,
        services: ServiceFlags::NONE, // I cheated here. 
        timestamp: 0,
        receiver: Address::new(
            &SocketAddr::new("127.0.0.1".parse().unwrap(), 18444),
            ServiceFlags::NETWORK,
        ),
        sender: Address::new(
            &SocketAddr::new("127.0.0.1".parse().unwrap(), 18444),
            ServiceFlags::NETWORK,
        ),
        nonce: 0,
        user_agent: "rust-seminar".to_string(),
        start_height: 200,
        relay: false,
    };

    let version_network_message =
        RawNetworkMessage::new(network.magic(), NetworkMessage::Version(version_message));

    println!("[+] Sending version message");
    stream
        .write_all(&serialize(&version_network_message))
        .unwrap();
    println!("[+] Sent version message");

    let mut buffer: Vec<u8> = Vec::new();
    let mut temp_buffer: Vec<u8> = vec![0; 4096];

    loop {
        match reader.read(&mut temp_buffer) {
            Ok(bytes_read) if bytes_read > 0 => {
                buffer.extend_from_slice(&temp_buffer[..bytes_read]);

                // Ensure partial and multiple messages are handled correctly
                while buffer.len() >= 24 { // Minimum Bitcoin message header size
                    if buffer.len() < 24 {
                        println!("[-] Not enough data for a full message header. Waiting for more data.");
                        break;
                    }

                    // Extract the payload size from the header
                    let payload_size = u32::from_le_bytes(buffer[16..20].try_into().unwrap()) as usize;
                    let total_message_size = 24 + payload_size; // Header size + payload size

                    if buffer.len() < total_message_size {
                        println!("[-] Incomplete message received. Waiting for more data.");
                        break; // Wait for more data
                    }

                    match deserialize::<RawNetworkMessage>(&buffer[..total_message_size]) {
                        Ok(msg) => {
                            match msg.payload() {
                                NetworkMessage::Version(version) => {
                                    println!("[+] Received Version: {:?}", version);

                                    // Perform handshake by sending Verack
                                    let verack_msg = RawNetworkMessage::new(
                                        network.magic(),
                                        NetworkMessage::Verack,
                                    );
                                    stream.write_all(&serialize(&verack_msg)).unwrap();
                                    println!("[+] Sent Verack");

                                    // Save service flags to the peer database
                                    if let Some(peer_info) = peers.get_mut(&target_addr) {
                                        peer_info.status = PeerStatus::ConnectedRecently;
                                        peer_info.last_connected = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
                                        println!("[+] Node supports service flags: {:?}", version.services);

                                        // Save updated peer info to the database
                                        save_peers_to_database(&conn, &peers).expect("Failed to save peers to database");
                                    }

                                    // Request address list
                                    let getaddr_msg = RawNetworkMessage::new(
                                        network.magic(),
                                        NetworkMessage::GetAddr,
                                    );
                                    stream.write_all(&serialize(&getaddr_msg)).unwrap();
                                    println!("[+] Sent Request Address");
                                }

                                NetworkMessage::Addr(addresses) => {
                                    println!("[+] Received addr message with {} addresses", addresses.len());
                                    for addr in addresses {
                                        println!("Address: {:?}", addr);

                                        // Convert Bitcoin address to SocketAddr
                                        if let Ok(socket_addr) = addr.1.socket_addr() {
                                            let peer_info = peers.entry(socket_addr).or_insert_with(|| PeerInfo::new(socket_addr));

                                            // Test connectivity to the peer
                                            let timeout_duration = std::time::Duration::from_secs(5);
                                            match TcpStream::connect_timeout(&socket_addr, timeout_duration) {
                                                Ok(_) => {
                                                    println!("[+] Successfully connected to peer: {}", socket_addr);
                                                    peer_info.last_connected = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
                                                    peer_info.status = PeerStatus::ConnectedRecently;

                                                    // Save updated peers to database immediately after a successful connection
                                                    save_peers_to_database(&conn, &peers).expect("Failed to save peers to database");
                                                }
                                                Err(_) => {
                                                    println!("[-] Failed to connect to peer: {}", socket_addr);
                                                    peer_info.status = PeerStatus::Unreachable;
                                                }
                                            }
                                        } else {
                                            println!("[-] Invalid address format: {:?}", addr);
                                        }
                                    }
                                }
                                NetworkMessage::Ping(nonce) => {
                                    println!("[+] Responding to Ping({})", nonce);
                                    let pong_msg = RawNetworkMessage::new(
                                        network.magic(),
                                        NetworkMessage::Pong(*nonce),
                                    );
                                    stream.write_all(&serialize(&pong_msg)).unwrap();
                                    println!("[+] Sent Pong");
                                }
                                _ => {
                                    println!("[?] Received unexpected message: {:?}", msg.payload().command());
                                }
                            }

                            buffer.drain(0..total_message_size); // Remove processed message from buffer
                        }
                        Err(e) => {
                            println!("[-] Failed to deserialize message. Error: {}. Raw buffer: {:?}", e, buffer);
                            buffer.clear(); // Clear buffer to avoid infinite loop
                            break;
                        }
                    }
                }
            }
            Ok(_) => {
                println!("[-] No data received, retrying...");
            }
            Err(e) => {
                println!("[-] Failed to read from stream: {}", e);
                break;
            }
        }
    }

    // Save peers to database before exiting
    println!("[+] Saving peers to database...");
    save_peers_to_database(&conn, &peers).expect("Failed to save peers to database");
}