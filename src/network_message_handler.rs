use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::{
    Network,
    consensus::{
        Decodable,
        encode::serialize,
    },
    io::Cursor,
    p2p::{
        Address, ServiceFlags,
        message::{NetworkMessage, RawNetworkMessage},
        message_network::VersionMessage,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{Event, LogLevel, LogMessage};
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub struct NetworkMessageHandler {
    pub network: Network,
    pub log_tx: Option<Sender<LogMessage>>,
}

impl NetworkMessageHandler {
    pub fn new(network: Network, log_tx: Option<Sender<LogMessage>>) -> Self {
        NetworkMessageHandler { network, log_tx }
    }

    fn build_version_message(&self, stream: &mut TcpStream) -> RawNetworkMessage {
        let version_message = VersionMessage {
            version: 70012, // 70012
            services: ServiceFlags::NONE,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            receiver: Address::new(&stream.peer_addr().unwrap(), ServiceFlags::NETWORK),
            sender: Address::new(&stream.local_addr().unwrap(), ServiceFlags::NETWORK),
            nonce: rand::random(),
            user_agent: "/rust-seminar:0.1.0/".to_string(),
            start_height: 0,
            relay: false,
        };

        RawNetworkMessage::new(
            self.network.magic(),
            NetworkMessage::Version(version_message),
        )
    }

    pub async fn send_version_message(&self, stream: &mut TcpStream) {
        let message = self.build_version_message(stream);
        let result = stream.write_all(&serialize(&message)).await;

        match result {
            Ok(_) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Custom("Version message sent".to_string()),
                        })
                        .await;
                }
            }
            Err(e) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Error,
                            event: Event::Custom(format!("Error sending version message: {:?}", e)),
                        })
                        .await;
                }
            }
        }
    }

    pub async fn send_verack_message(&self, stream: &mut TcpStream) {
        let verack_message = RawNetworkMessage::new(self.network.magic(), NetworkMessage::Verack);
        let result = stream.write_all(&serialize(&verack_message)).await;

        match result {
            Ok(_) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Custom("Verack message sent".to_string()),
                        })
                        .await;
                }
            }
            Err(e) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Error,
                            event: Event::Custom(format!("Error sending verack message: {:?}", e)),
                        })
                        .await;
                }
            }
        }
    }

    pub async fn send_pong_message(&self, stream: &mut TcpStream, nonce: u64) {
        let pong_message =
            RawNetworkMessage::new(self.network.magic(), NetworkMessage::Pong(nonce));
        let result = stream.write_all(&serialize(&pong_message)).await;

        match result {
            Ok(_) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Custom("Pong message sent".to_string()),
                        })
                        .await;
                }
            }
            Err(e) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Error,
                            event: Event::Custom(format!("Error sending pong message: {:?}", e)),
                        })
                        .await;
                }
            }
        }
    }

    pub async fn send_getaddr_message(&self, stream: &mut TcpStream) {
        let getaddr_message = RawNetworkMessage::new(self.network.magic(), NetworkMessage::GetAddr);
        let result = stream.write_all(&serialize(&getaddr_message)).await;

        match result {
            Ok(_) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Info,
                            event: Event::Custom("GetAddr message sent".to_string()),
                        })
                        .await;
                }
            }
            Err(e) => {
                if let Some(log_tx) = &self.log_tx {
                    let _ = log_tx
                        .send(LogMessage {
                            level: LogLevel::Error,
                            event: Event::Custom(format!("Error sending getaddr message: {:?}", e)),
                        })
                        .await;
                }
            }
        }
    }

    pub async fn receive_message(
        &self,
        stream: &mut TcpStream,
    ) -> Result<RawNetworkMessage, std::io::Error> {
        const HEADER_LEN: usize = 24;
        let mut header_buf = [0u8; HEADER_LEN];
        stream.read_exact(&mut header_buf).await?;

        let payload_len = {
            let mut cursor = Cursor::new(&header_buf[16..20]);
            u32::consensus_decode(&mut cursor)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        } as usize;

        let mut payload_buf = vec![0u8; payload_len];
        stream.read_exact(&mut payload_buf).await?;

        let mut full_buf = Vec::with_capacity(HEADER_LEN + payload_len);
        full_buf.extend_from_slice(&header_buf);
        full_buf.extend_from_slice(&payload_buf);

        let mut cursor = Cursor::new(full_buf);
        let message = RawNetworkMessage::consensus_decode(&mut cursor)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(message)
    }
}
