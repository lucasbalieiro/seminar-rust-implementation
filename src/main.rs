use std::net::{IpAddr, SocketAddr};

use clap::Parser;
use seminar_rust_implementation::{cli::Cli, crawler::Crawler, database, node::Node};
use database::Database;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    tracing::info!("Starting the crawler...");

    Database::new("peers.db".to_string());

    let args = Cli::parse();

    let fallback_node = SocketAddr::new(IpAddr::V4(args.host.parse().unwrap()), args.port);

    let fallback_node = Node::new(fallback_node, args.network);

    // TODO: check if theres no nodes to be crawled in the persistent storage, if not then use the fallback node

    let mut crawler = Crawler::new(fallback_node, args.timeout);

    crawler.crawl_peer().await;
}
