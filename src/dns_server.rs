use std::net::Ipv4Addr;
use rand::seq::SliceRandom;
use tokio::net::UdpSocket;
use crate::database::Database;
use crate::logger::{LogLevel, LogMessage, Event};

const MAX_DNS_PACKET_SIZE: usize = 512;
const MAX_ANSWERS: usize = 10;
const DNS_PORT: u16 = 1053; // Use 53 for production, 1053 for dev
const SEED_DOMAIN: &str = "seed.lucasbalieiro.dev"; // TODO: Needs to be set via CLI

// DNS header is always 12 bytes
// http://datatracker.ietf.org/doc/html/rfc1035#section-4.1.1
struct DnsHeader {
    id: u16,
    flags: u16,
    qdcount: u16,
    ancount: u16,
    nscount: u16,
    arcount: u16,
}

impl DnsHeader {
    fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < 12 { return None; }
        Some(DnsHeader {
            id: u16::from_be_bytes([buf[0], buf[1]]),
            flags: u16::from_be_bytes([buf[2], buf[3]]),
            qdcount: u16::from_be_bytes([buf[4], buf[5]]),
            ancount: u16::from_be_bytes([buf[6], buf[7]]),
            nscount: u16::from_be_bytes([buf[8], buf[9]]),
            arcount: u16::from_be_bytes([buf[10], buf[11]]),
        })
    }
    fn write(&self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.id.to_be_bytes());
        buf[2..4].copy_from_slice(&self.flags.to_be_bytes());
        buf[4..6].copy_from_slice(&self.qdcount.to_be_bytes());
        buf[6..8].copy_from_slice(&self.ancount.to_be_bytes());
        buf[8..10].copy_from_slice(&self.nscount.to_be_bytes());
        buf[10..12].copy_from_slice(&self.arcount.to_be_bytes());
    }
}

// Parse a DNS name from the buffer, return (name, next_offset)
fn parse_qname(buf: &[u8], mut offset: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    while offset < buf.len() {
        let len = buf[offset] as usize;
        if len == 0 { return Some((labels.join("."), offset + 1)); }
        offset += 1;
        if offset + len > buf.len() { return None; }
        labels.push(String::from_utf8_lossy(&buf[offset..offset+len]).to_string());
        offset += len;
    }
    None
}

// Write a DNS name to the buffer, return next offset
fn write_qname(buf: &mut [u8], mut offset: usize, name: &str) -> usize {
    for label in name.trim_end_matches('.').split('.') {
        let len = label.len();
        buf[offset] = len as u8;
        offset += 1;
        buf[offset..offset+len].copy_from_slice(label.as_bytes());
        offset += len;
    }
    buf[offset] = 0;
    offset + 1
}

// DNS Question: (qname, qtype, qclass)
struct DnsQuestion {
    qname: String,
    qtype: u16,
    qclass: u16,
    end_offset: usize,
}

fn parse_question(buf: &[u8], offset: usize) -> Option<DnsQuestion> {
    let (qname, mut pos) = parse_qname(buf, offset)?;
    if pos + 4 > buf.len() { return None; }
    let qtype = u16::from_be_bytes([buf[pos], buf[pos+1]]);
    let qclass = u16::from_be_bytes([buf[pos+2], buf[pos+3]]);
    Some(DnsQuestion { qname, qtype, qclass, end_offset: pos + 4 })
}

// Write a DNS A record answer
fn write_a_record(buf: &mut [u8], mut offset: usize, name: &str, ip: Ipv4Addr) -> usize {
    offset = write_qname(buf, offset, name);
    buf[offset..offset+2].copy_from_slice(&1u16.to_be_bytes()); // TYPE A
    buf[offset+2..offset+4].copy_from_slice(&1u16.to_be_bytes()); // CLASS IN
    buf[offset+4..offset+8].copy_from_slice(&60u32.to_be_bytes()); // TTL 60s
    buf[offset+8..offset+10].copy_from_slice(&4u16.to_be_bytes()); // RDLENGTH
    buf[offset+10..offset+14].copy_from_slice(&ip.octets()); // RDATA
    offset + 14
}

pub async fn run_dns_server(db: Database, log_tx: Option<tokio::sync::mpsc::Sender<LogMessage>>) {
    let socket = UdpSocket::bind(("0.0.0.0", DNS_PORT)).await.expect("Failed to bind UDP port");
    if let Some(ref tx) = log_tx {
        let _ = tx.send(LogMessage { level: LogLevel::Info, event: Event::Custom(format!("DNS server listening on UDP port {}", DNS_PORT)) }).await;
    }
    let mut buf = [0u8; MAX_DNS_PACKET_SIZE];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(_) => continue,
        };
        let req = &buf[..len];
        // Parse header
        let header = match DnsHeader::parse(req) {
            Some(h) => h,
            None => continue,
        };
        if header.qdcount != 1 { continue; }
        // Parse question
        let question = match parse_question(req, 12) {
            Some(q) => q,
            None => continue,
        };
        // Log the query
        if let Some(ref tx) = log_tx {
            let _ = tx.send(LogMessage { level: LogLevel::Info, event: Event::Custom(format!("DNS query from {}: {} type {} class {}", src, question.qname, question.qtype, question.qclass)) }).await;
        }
        // Only answer A/IN for our domain
        let is_a = question.qtype == 1 && question.qclass == 1 && question.qname == SEED_DOMAIN;
        let mut resp = [0u8; MAX_DNS_PACKET_SIZE];
        let mut ancount = 0u16;
        let mut resp_len = 0;
        // Copy header, set response flags
        let mut resp_header = header;
        resp_header.flags = 0x8180; // QR=1, OPCODE=0, AA=1, TC=0, RD=1, RA=1, RCODE=0
        resp_header.ancount = 0;
        resp_header.nscount = 0;
        resp_header.arcount = 0;
        resp_header.write(&mut resp);
        resp_len = 12;
        // Write question back
        resp_len = write_qname(&mut resp, resp_len, &question.qname);
        resp[resp_len..resp_len+2].copy_from_slice(&question.qtype.to_be_bytes());
        resp[resp_len+2..resp_len+4].copy_from_slice(&question.qclass.to_be_bytes());
        resp_len += 4;
        if is_a {
            // Get up to 10 random IPv4 peers
            let mut nodes = db.get_nodes(MAX_ANSWERS as u8);
            nodes.shuffle(&mut rand::rng());
            let peers = nodes.into_iter().filter_map(|n| n.address.parse::<Ipv4Addr>().ok()).collect::<Vec<_>>();
            for ip in peers.iter().take(MAX_ANSWERS) {
                resp_len = write_a_record(&mut resp, resp_len, &question.qname, *ip);
                ancount += 1;
            }
            resp_header.ancount = ancount;
            resp_header.write(&mut resp);
        } else {
            // Not implemented for other types
            resp_header.flags = 0x8184; // RCODE=4 (Not Implemented)
            resp_header.write(&mut resp);
        }
        let _ = socket.send_to(&resp[..resp_len], src).await;
        if let Some(ref tx) = log_tx {
            let _ = tx.send(LogMessage { level: LogLevel::Info, event: Event::Custom(format!("DNS response sent to {}: {} answers", src, ancount)) }).await;
        }
    }
}
