use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use bitcoin::p2p::Address;
use clap::Parser;
use database::Database;
use seminar_rust_implementation::{cli::Cli, crawler::Crawler, database, node::Node};
use tokio::sync::mpsc;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting the crawler...");

    let (tx, rx) = mpsc::channel::<Vec<(u32, Address)>>(100);

    let db = Database::new("peers.db".to_string());
    let mut db_receiver = database::DatabaseReceiver::new(rx, db);
    let db_handle = tokio::spawn(async move {
        db_receiver.run_rx_loop().await;
    });

    let args = Cli::parse();

    // Create a new Database instance for reading (separate from the one used by the receiver)
    let db_read = Database::new("peers.db".to_string());
    let seed_peers = db_read
        .get_nodes(args.threads)
        .into_iter()
        .map(|info| info.into())
        .collect::<Vec<Node>>();

    let fallback_node = SocketAddr::new(IpAddr::V4(args.host.parse().unwrap()), args.port);

    let fallback_node = Node::new(fallback_node, args.network);

    let mut crawler = Crawler::new(fallback_node, args.timeout, tx.clone());

    if seed_peers.is_empty() {
        tracing::info!("No seed peers found. Using fallback node.");
        crawler.node = fallback_node.clone();
        tokio::spawn(async move {
            crawler.crawl_peer().await;
        })
        .await
        .unwrap();
    } else {
        tracing::info!("Using seed peers.");
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
