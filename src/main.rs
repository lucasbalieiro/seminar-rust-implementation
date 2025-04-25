mod cli;
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::network::Network;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::ServiceFlags;
use clap::Parser;
use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};

fn main() {
    // Parse command-line arguments
    let args = cli::Cli::parse();

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
                                    let verack_msg = RawNetworkMessage::new(
                                        network.magic(),
                                        NetworkMessage::Verack,
                                    );
                                    stream.write_all(&serialize(&verack_msg)).unwrap();
                                    println!("[+] Sent Verack");


                                    //request address
                                    let getaddr_msg = RawNetworkMessage::new(
                                        network.magic(),
                                        NetworkMessage::GetAddr,
                                    );

                                    stream
                                        .write_all(&serialize(&getaddr_msg))
                                        .unwrap();

                                    println!("[+] Sent Request Address");
                                }


                                NetworkMessage::Addr(addresses) => {
                                    println!("[+] Received addr message with {} addresses", addresses.len());
                                    for addr in addresses {
                                        println!("Address: {:?}", addr);
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
                                    println!("[?] Received unexpected message: {:?}", msg.payload());
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
}