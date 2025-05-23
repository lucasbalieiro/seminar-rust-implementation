use crate::network_message_handler::NetworkMessageHandler;
use crate::node::Node;
use crate::{Event, LogLevel, LogMessage};
use bitcoin::p2p::message::NetworkMessage;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Clone)]
pub struct Crawler {
    pub node: Node,
    message_handler: NetworkMessageHandler,
    timeout: Duration,
    tx: tokio::sync::mpsc::Sender<Vec<(u32, bitcoin::p2p::Address)>>,
    pub log_tx: Option<tokio::sync::mpsc::Sender<LogMessage>>,
}

impl Crawler {
    pub fn new(
        node: Node,
        timeout: u64,
        tx: tokio::sync::mpsc::Sender<Vec<(u32, bitcoin::p2p::Address)>>,
        log_tx: Option<tokio::sync::mpsc::Sender<LogMessage>>,
    ) -> Self {
        Crawler {
            node,
            message_handler: NetworkMessageHandler::new(node.network, log_tx.clone()),
            timeout: Duration::from_secs(timeout),
            tx,
            log_tx,
        }
    }

    pub async fn crawl_peer(&mut self) {
        let stream = timeout(self.timeout, TcpStream::connect(self.node.address)).await;
        match stream.unwrap() {
            Ok(mut tcpstream) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Connected(self.node.address),
                        })
                        .await;
                }
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
                        if let Some(log_tx) = &self.log_tx {
                            let _ = log_tx
                                .send(LogMessage {
                                    level: LogLevel::Error,
                                    event: Event::Custom(format!(
                                        "Error receiving message: {:?}",
                                        received_message
                                    )),
                                })
                                .await;
                        }
                        break;
                    }
                    let received_message = received_message.unwrap().unwrap();

                    let cmd = received_message.cmd().to_owned();
                    match received_message.into_payload() {
                        NetworkMessage::Version(version_message) => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(format!(
                                            "Received version message from {}: {:?}",
                                            self.node.address, version_message
                                        )),
                                    })
                                    .await;
                            }
                            self.message_handler
                                .send_verack_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Verack => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(format!(
                                            "Received verack message from {}",
                                            self.node.address
                                        )),
                                    })
                                    .await;
                            }
                            self.message_handler
                                .send_getaddr_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Ping(nonce) => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(format!(
                                            "Received ping message from {}: {}",
                                            self.node.address, nonce
                                        )),
                                    })
                                    .await;
                            }
                            self.message_handler
                                .send_pong_message(&mut tcpstream, nonce)
                                .await;
                        }
                        NetworkMessage::Pong(nonce) => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(format!(
                                            "Received pong message from {}: {}",
                                            self.node.address, nonce
                                        )),
                                    })
                                    .await;
                            }
                            self.message_handler
                                .send_getaddr_message(&mut tcpstream)
                                .await;
                        }
                        NetworkMessage::Addr(addr_message) => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(format!(
                                            "Received addr message from {}",
                                            self.node.address
                                        )),
                                    })
                                    .await;
                            }
                            self.tx.send(addr_message).await.unwrap();
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Info,
                                        event: Event::Custom(
                                            "Sent addr message to the database".to_string(),
                                        ),
                                    })
                                    .await;
                            }
                            break;
                        }
                        _ => {
                            if let Some(log_tx) = &self.log_tx {
                                let _ = log_tx
                                    .send(LogMessage {
                                        level: LogLevel::Warn,
                                        event: Event::Custom(format!(
                                            "Unhandled message type: {}",
                                            cmd
                                        )),
                                    })
                                    .await;
                            }
                        }
                    }
                }
                // Properly shutdown the TcpStream for writing (best effort, not async)
                use tokio::io::AsyncWriteExt;
                let _ = tcpstream.shutdown().await;
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Custom(format!(
                                "Connection closed to {}",
                                self.node.address
                            )),
                        })
                        .await;
                }
            }
            Err(e) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Error,
                            event: Event::FailedConnection(self.node.address, e.to_string()),
                        })
                        .await;
                }
            }
        }
    }
}
