use rusqlite::Connection;

use crate::node::NodeInfo;

pub struct Database {
    filename: String,
    connection: Option<Connection>,
}

impl Database {
    pub fn new(filename: String) {
        let mut db = Database {
            filename,
            connection: None,
        };
        db.connect();
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
        ).unwrap();
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
                ).unwrap();
            }
            tx.commit().unwrap();
        }
    }    
}