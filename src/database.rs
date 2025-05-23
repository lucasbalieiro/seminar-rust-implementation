use crate::{Event, LogLevel, LogMessage};
use rusqlite::Connection;
use tokio::sync::mpsc::Sender;

use crate::node::NodeInfo;

pub struct Database {
    filename: String,
    connection: Option<Connection>,
}

pub struct DatabaseReceiver {
    rx: tokio::sync::mpsc::Receiver<Vec<(u32, bitcoin::p2p::Address)>>,
    db: Database,
    pub log_tx: Option<tokio::sync::mpsc::Sender<LogMessage>>,
}

impl Database {
    pub fn new(filename: String) -> Database {
        let mut db = Database {
            filename,
            connection: None,
        };
        db.connect();
        db
    }

    fn connect(&mut self) {
        let conn = Connection::open(&self.filename).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS nodes (
                address TEXT NOT NULL UNIQUE,
                port INTEGER NOT NULL,
                last_seen INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        self.connection = Some(conn);
    }

    pub fn bulk_insert_or_update_nodes(&mut self, nodes: &[NodeInfo]) {
        if let Some(conn) = &mut self.connection {
            let tx = conn.transaction().unwrap();
            for node in nodes {
                tx.execute(
                    "INSERT INTO nodes (address, port, last_seen) VALUES (?1, ?2, ?3)
                    ON CONFLICT(address) DO UPDATE SET last_seen = excluded.last_seen",
                    rusqlite::params![node.address, node.port, node.last_seen],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
    }

    pub fn get_nodes(&self, limit: u8) -> Vec<NodeInfo> {
        if let Some(conn) = &self.connection {
            let mut stmt = conn
                .prepare(
                    "SELECT address, port, last_seen FROM nodes ORDER BY last_seen DESC LIMIT ?1",
                )
                .unwrap();
            let nodes_iter = stmt
                .query_map([limit], |row| {
                    Ok(NodeInfo {
                        address: row.get(0)?,
                        port: row.get(1)?,
                        last_seen: row.get(2)?,
                    })
                })
                .unwrap();

            nodes_iter.filter_map(Result::ok).collect()
        } else {
            vec![]
        }
    }
}

impl DatabaseReceiver {
    pub fn new(
        rx: tokio::sync::mpsc::Receiver<Vec<(u32, bitcoin::p2p::Address)>>,
        db: Database,
        log_tx: Option<Sender<LogMessage>>,
    ) -> Self {
        Self { rx, db, log_tx }
    }

    pub async fn run_rx_loop(&mut self) {
        use crate::node::NodeInfo;
        while let Some(batch) = self.rx.recv().await {
            let nodes: Vec<NodeInfo> = batch
                .into_iter()
                .map(|(last_seen, address)| NodeInfo {
                    address: address.socket_addr().unwrap().ip().to_string(),
                    port: address.port,
                    last_seen,
                })
                .collect();
            self.db.bulk_insert_or_update_nodes(&nodes);
            if let Some(log_tx) = &self.log_tx {
                let _ = log_tx
                    .send(LogMessage {
                        level: LogLevel::Info,
                        event: Event::SavedToDisk(nodes.len()),
                    })
                    .await;
            }
        }
    }
}

mod tests {
    use super::*;    

    #[tokio::test]
    async fn test_database() {
        let mut db = Database::new("test.db".to_string());

        let nodes = vec![
            NodeInfo {
                address: "127.0.0.1".to_string(),
                port: 8080,
                last_seen: 1234567891,
            },
            NodeInfo {
                address: "192.168.1.1".to_string(),
                port: 8080,
                last_seen: 1234567890,
            },
        ];
        db.bulk_insert_or_update_nodes(&nodes);
    }
    #[test]
    fn test_get_nodes() {
        let db = Database::new("test.db".to_string());
        let nodes = db.get_nodes(2);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].address, "127.0.0.1");
        assert_eq!(nodes[0].port, 8080);
        assert_eq!(nodes[0].last_seen, 1234567891);
        assert_eq!(nodes[1].address, "192.168.1.1");
        assert_eq!(nodes[1].port, 8080);
        assert_eq!(nodes[1].last_seen, 1234567890);
    }
}
