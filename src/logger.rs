use std::net::SocketAddr;
use tokio::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(SocketAddr),
    FailedConnection(SocketAddr, String),
    PeerDiscovered(SocketAddr),
    SavedToDisk(usize),
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct LogMessage {
    pub level: LogLevel,
    pub event: Event,
}

pub async fn logger_task(mut rx: Receiver<LogMessage>, min_level: LogLevel) {
    while let Some(msg) = rx.recv().await {
        if msg.level >= min_level {
            match msg.level {
                LogLevel::Error | LogLevel::Warn => eprintln!("[{:?}] {:?}", msg.level, msg.event),
                _ => println!("[{:?}] {:?}", msg.level, msg.event),
            }
        }
    }
}
