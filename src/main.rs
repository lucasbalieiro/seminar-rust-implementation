use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use bitcoin::p2p;
use clap::Parser;
use database::Database;
use seminar_rust_implementation::{
    dns_server, Event, LogLevel, LogMessage, cli::Cli, crawler::Crawler, database, logger::logger_task,
    node::Node,
};
use tokio::sync::{mpsc, RwLock};

fn parse_level(s: &str) -> LogLevel {
    match s.to_lowercase().as_str() {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[tokio::main]
async fn main() {
    let (tx, rx) = mpsc::channel::<Vec<(u32, p2p::Address)>>(100);

    let db_read = Database::new("peers.db".to_string());
    let db_arc = Arc::new(RwLock::new(db_read));

    let cli = Cli::parse();
    let min_level = parse_level(&cli.verbosity);

    let (log_tx, log_rx) = mpsc::channel::<LogMessage>(100);
    tokio::spawn(logger_task(log_rx, min_level));

    // Spawn DNS server (create a new Database instance inside the task)
    let dns_log = Some(log_tx.clone());
    tokio::spawn(async move {
        let db = Database::new("peers.db".to_string());
        dns_server::run_dns_server(db, dns_log).await;
    });

    let mut db_receiver = database::DatabaseReceiver::new(rx, Database::new("peers.db".to_string()), Some(log_tx.clone()));
    let db_handle = tokio::spawn(async move {
        db_receiver.run_rx_loop().await;
    });

    // Create a new Database instance for reading (separate from the one used by the receiver)
    let seed_peers = db_arc
        .read()
        .await
        .get_nodes(cli.threads)
        .into_iter()
        .map(|info| info.into())
        .collect::<Vec<Node>>();

    let fallback_node = SocketAddr::new(IpAddr::V4(cli.host.parse().unwrap()), cli.port);

    let fallback_node = Node::new(fallback_node, cli.network.clone());

    let mut crawler = Crawler::new(fallback_node, cli.timeout, tx.clone(), Some(log_tx.clone()));

    if seed_peers.is_empty() {
        let _ = log_tx
            .send(LogMessage {
                level: LogLevel::Info,
                event: Event::Custom("No seed peers found. Using fallback node.".to_string()),
            })
            .await;
        crawler.node = fallback_node.clone();
        tokio::spawn(async move {
            crawler.crawl_peer().await;
        })
        .await
        .unwrap();
    } else {
        let _ = log_tx
            .send(LogMessage {
                level: LogLevel::Info,
                event: Event::Custom("Using seed peers.".to_string()),
            })
            .await;
        let mut handles = Vec::new();
        for seed_peer in seed_peers {
            let mut crawler = crawler.clone();
            crawler.node = seed_peer;
            let handle = tokio::spawn(async move {
                crawler.crawl_peer().await;
            });
            handles.push(handle);
        }
        // Wait for all spawned tasks to finish
        for handle in handles {
            let _ = handle.await;
        }
    }
    // Now all tx clones are dropped, so the receiver will exit after processing all messages
    let _ = db_handle.await;
}
