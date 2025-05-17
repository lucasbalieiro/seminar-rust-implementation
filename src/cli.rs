use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "Bitcoin Seeder")]
#[command(about = "A CLI for interacting with the Bitcoin Seeder", long_about = None)]
pub struct Cli {
    /// Host to connect to (default: seed.bitcoin.sipa.be)
    #[arg(long, default_value = "seed.bitcoin.sipa.be")]
    pub host: String,

    /// Port to connect to (default: 8333)
    #[arg(long, default_value_t = 8333)]
    pub port: u16,

    /// Network to connect to (default: mainnet)
    #[arg(long, default_value = "mainnet")]
    pub network: String,

    /// Number of crawling threads
    #[arg(long)]
    pub threads: Option<u8>,

    /// Timeout for connection attempts in seconds (default: 10, minimum: 1)
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,
}