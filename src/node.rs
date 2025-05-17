use bitcoin::p2p::Address;
use std::{fmt::Debug, net::{IpAddr, SocketAddr}};

use bitcoin::Network;

#[derive(Clone, Copy)]
pub struct Node {
    pub address: SocketAddr,
    pub network: Network,
}

impl Node {
    pub fn new(address: SocketAddr, network: String) -> Self {
        let network = Self::parse_network(&network);
        Node { address, network }
    }

    fn parse_network(network: &str) -> Network {
        match network {
            "mainnet" => Network::Bitcoin,
            "testnet" => Network::Testnet,
            "regtest" => Network::Regtest,
            "testnet4" => Network::Testnet4,
            _ => panic!("Invalid network specified"),
        }
    }
}

pub struct NodeInfo {
    pub last_seen: u32,
    pub address: String,
    pub port: u16,
}

impl From<(u32, Address)> for NodeInfo {
    fn from((last_seen, node): (u32, Address)) -> Self {

        let address = SocketAddr::new(IpAddr::from(node.address), node.port);
        NodeInfo {
            last_seen,
            address: address.ip().to_string(),
            port: address.port(),
        }
    }
}
