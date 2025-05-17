use std::time::{self, Duration};

use bitcoin::p2p::message::NetworkMessage;
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    time::{sleep, timeout},
};
use tracing::{error, info, warn};

use crate::{network_message_handler::NetworkMessageHandler, node::Node};

pub struct Crawler {
    pub node: Node,
    message_handler: NetworkMessageHandler,
    timeout: Duration,
}

impl Crawler {
    pub fn new(node: Node, timeout: u64) -> Self {
        Crawler {
            node,
            message_handler: NetworkMessageHandler::new(node.network),
            timeout: Duration::from_secs(timeout),
        }
    }

    pub async fn crawl_peer(&mut self) {
        let stream = timeout(self.timeout, TcpStream::connect(self.node.address)).await;
        match stream.unwrap() {
            Ok(mut tcpstream) => {
                info!(
                    "[+] Connected to {}. Performing handshake...",
                    self.node.address
                );
                self.message_handler
                    .send_version_message(&mut tcpstream)
                    .await;
                loop {
                    let received_message = timeout(
                        self.timeout,
                        self.message_handler.receive_message(&mut tcpstream),
                    )
                    .await;
                    
                    if received_message.is_err() {
                        error!("[-] Error receiving message: {:?}", received_message);
                        break;
                    }
                    let received_message = received_message.unwrap().unwrap();

                    let cmd = received_message.cmd().to_owned();
                    match received_message.into_payload() {
                        NetworkMessage::Version(version_message) => {
                            info!(
                                "[+] Received version message from {}: {:?}",
                                self.node.address, version_message
                            );
                            self.message_handler
                                .send_verack_message(&mut tcpstream)
                                .await;

                            self.message_handler
                                .send_getaddr_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Verack => {
                            info!("[+] Received verack message from {}", self.node.address);

                            self.message_handler
                                .send_getaddr_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Ping(nonce) => {
                            info!(
                                "[+] Received ping message from {}: {}",
                                self.node.address, nonce
                            );
                            self.message_handler
                                .send_pong_message(&mut tcpstream, nonce)
                                .await;
                        }
                        NetworkMessage::Pong(nonce) => {
                            info!(
                                "[+] Received pong message from {}: {}",
                                self.node.address, nonce
                            );
                            self.message_handler
                                .send_getaddr_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Addr(addr_message) => {
                            info!(
                                "[+] Received addr message from {}: {:?}",
                                self.node.address, addr_message
                            );
                            break;
                        }
                        _ => {
                            warn!("[-] Unhandled message type: {}", cmd);
                        }
                    }
                }
                let _ = tcpstream.shutdown().await;
                info!("[+] Connection closed to {}", self.node.address);
            }
            Err(e) => {
                error!("[-] Error connecting to {}: {:?}", self.node.address, e);
            }
        }
    }
}
