use bitcoin::p2p::Address;
use std::net::{IpAddr, SocketAddr};

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

impl Into<Node> for NodeInfo {
    fn into(self) -> Node {
        let ip: IpAddr = self.address.parse().unwrap();
        let address = SocketAddr::new(ip, self.port);
        Node::new(address, "mainnet".to_string())
    }
}

mod tests {
    use super::*;

    #[test]
    fn test_convert_from_node_info_to_node() {
        let node_info = NodeInfo {
            last_seen: 1234567890,
            address: "127.0.0.1".to_string(),
            port: 8333,
        };
        let node: Node = node_info.into();
        assert_eq!(
            node.address,
            SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 8333)
        );
        assert_eq!(node.network, Network::Bitcoin);
    }
}
