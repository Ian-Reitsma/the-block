use std::fs;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str;
use std::time::Duration;

use rand::{rngs::StdRng, Rng, SeedableRng};

use crate::{block_on, timeout};

use super::UdpSocket;

const DNS_PORT: u16 = 53;
const MAX_MESSAGE_SIZE: usize = 512;
const QUERY_FLAGS: u16 = 0x0100;
const CLASS_IN: u16 = 1;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;

/// Result of an SRV lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String,
}

/// Perform a blocking TXT lookup using the in-house runtime.
pub fn lookup_txt(domain: &str) -> io::Result<Vec<String>> {
    block_on(async_lookup(domain, TYPE_TXT)).and_then(|bytes| parse_txt_records(domain, &bytes))
}

/// Perform a blocking SRV lookup using the in-house runtime.
pub fn lookup_srv(record: &str) -> io::Result<Vec<SrvRecord>> {
    block_on(async_lookup(record, TYPE_SRV)).and_then(|bytes| parse_srv_records(record, &bytes))
}

async fn async_lookup(name: &str, record_type: u16) -> io::Result<Vec<u8>> {
    let query_name = name.trim().trim_end_matches('.');
    if query_name.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "empty dns query"));
    }

    let servers = system_nameservers();
    if servers.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            "no nameservers discovered for dns lookup",
        ));
    }

    let mut rng = StdRng::from_entropy();
    let query_id: u16 = rng.gen();
    let payload = build_query(query_name, record_type, query_id)?;

    let mut last_error: Option<io::Error> = None;
    for server in servers {
        let bind_addr = match server {
            SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        };
        let mut socket = match UdpSocket::bind(bind_addr).await {
            Ok(sock) => sock,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };

        if let Err(err) = socket.send_to(&payload, server).await {
            last_error = Some(err);
            continue;
        }

        let mut buf = [0u8; MAX_MESSAGE_SIZE];
        match timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _addr))) => {
                if len < 12 {
                    last_error = Some(io::Error::new(
                        ErrorKind::InvalidData,
                        "dns response shorter than header",
                    ));
                    continue;
                }
                let response = buf[..len].to_vec();
                if !matches_id(&response, query_id) {
                    last_error = Some(io::Error::new(
                        ErrorKind::InvalidData,
                        "dns response id mismatch",
                    ));
                    continue;
                }
                if truncated(&response) {
                    last_error = Some(io::Error::new(ErrorKind::Other, "dns response truncated"));
                    continue;
                }
                if let Err(err) = ensure_success(&response) {
                    last_error = Some(err);
                    continue;
                }
                return Ok(response);
            }
            Ok(Err(err)) => {
                last_error = Some(err);
            }
            Err(_) => {
                last_error = Some(io::Error::new(ErrorKind::TimedOut, "dns query timed out"));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(ErrorKind::TimedOut, "dns query exhausted all nameservers")
    }))
}

fn build_query(name: &str, record_type: u16, id: u16) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&id.to_be_bytes());
    buf.extend_from_slice(&QUERY_FLAGS.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    buf.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    buf.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    buf.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    encode_name(name, &mut buf)?;
    buf.extend_from_slice(&record_type.to_be_bytes());
    buf.extend_from_slice(&CLASS_IN.to_be_bytes());

    Ok(buf)
}

fn encode_name(name: &str, buf: &mut Vec<u8>) -> io::Result<()> {
    let trimmed = name.trim_matches('.');
    if trimmed.is_empty() {
        buf.push(0);
        return Ok(());
    }

    for label in trimmed.split('.') {
        let bytes = label.as_bytes();
        if bytes.len() > 63 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("dns label '{label}' exceeds 63 octets"),
            ));
        }
        buf.push(bytes.len() as u8);
        buf.extend_from_slice(bytes);
    }
    buf.push(0);
    Ok(())
}

fn matches_id(buf: &[u8], id: u16) -> bool {
    buf.len() >= 2 && u16::from_be_bytes([buf[0], buf[1]]) == id
}

fn truncated(buf: &[u8]) -> bool {
    buf.len() >= 4 && (u16::from_be_bytes([buf[2], buf[3]]) & 0x0200) != 0
}

fn ensure_success(buf: &[u8]) -> io::Result<()> {
    if buf.len() < 4 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "dns response missing header",
        ));
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    if (flags & 0x8000) == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "dns response missing qr flag",
        ));
    }
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(io::Error::new(
            ErrorKind::Other,
            format!("dns server returned error code {rcode}"),
        ));
    }
    Ok(())
}

fn parse_header_counts(buf: &[u8]) -> io::Result<(u16, u16, u16, u16)> {
    if buf.len() < 12 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "dns response shorter than header",
        ));
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]);
    let ancount = u16::from_be_bytes([buf[6], buf[7]]);
    let nscount = u16::from_be_bytes([buf[8], buf[9]]);
    let arcount = u16::from_be_bytes([buf[10], buf[11]]);
    Ok((qdcount, ancount, nscount, arcount))
}

fn parse_txt_records(_name: &str, buf: &[u8]) -> io::Result<Vec<String>> {
    let (qdcount, ancount, nscount, arcount) = parse_header_counts(buf)?;
    let mut offset = 12usize;
    for _ in 0..qdcount {
        let (_, next) = parse_name(buf, offset, 0)?;
        offset = next;
        if offset + 4 > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns question truncated",
            ));
        }
        offset += 4; // type + class
    }

    let mut txts = Vec::new();
    for _ in 0..ancount {
        let (_, next) = parse_name(buf, offset, 0)?;
        offset = next;
        if offset + 10 > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns answer header truncated",
            ));
        }
        let rr_type = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let rr_class = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlength = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlength > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns answer rdata truncated",
            ));
        }
        if rr_type == TYPE_TXT && rr_class == CLASS_IN {
            let mut cursor = offset;
            let end = offset + rdlength;
            while cursor < end {
                let len = buf[cursor] as usize;
                cursor += 1;
                if cursor + len > end {
                    break;
                }
                if let Ok(text) = str::from_utf8(&buf[cursor..cursor + len]) {
                    txts.push(text.to_string());
                }
                cursor += len;
            }
        }
        offset += rdlength;
    }

    skip_records(buf, &mut offset, nscount)?;
    skip_records(buf, &mut offset, arcount)?;

    Ok(txts)
}

fn parse_srv_records(_name: &str, buf: &[u8]) -> io::Result<Vec<SrvRecord>> {
    let (qdcount, ancount, nscount, arcount) = parse_header_counts(buf)?;
    let mut offset = 12usize;
    for _ in 0..qdcount {
        let (_, next) = parse_name(buf, offset, 0)?;
        offset = next;
        if offset + 4 > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns question truncated",
            ));
        }
        offset += 4;
    }

    let mut records = Vec::new();
    for _ in 0..ancount {
        let (_, next) = parse_name(buf, offset, 0)?;
        offset = next;
        if offset + 10 > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns answer header truncated",
            ));
        }
        let rr_type = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let rr_class = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlength = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlength > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns answer rdata truncated",
            ));
        }
        if rr_type == TYPE_SRV && rr_class == CLASS_IN && rdlength >= 6 {
            let priority = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
            let weight = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
            let port = u16::from_be_bytes([buf[offset + 4], buf[offset + 5]]);
            let (target, _) = parse_name(buf, offset + 6, 0)?;
            records.push(SrvRecord {
                priority,
                weight,
                port,
                target,
            });
        }
        offset += rdlength;
    }

    skip_records(buf, &mut offset, nscount)?;
    skip_records(buf, &mut offset, arcount)?;

    Ok(records)
}

fn skip_records(buf: &[u8], offset: &mut usize, count: u16) -> io::Result<()> {
    for _ in 0..count {
        let (_, next) = parse_name(buf, *offset, 0)?;
        *offset = next;
        if *offset + 10 > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns record header truncated",
            ));
        }
        let rdlength = u16::from_be_bytes([buf[*offset + 8], buf[*offset + 9]]) as usize;
        *offset += 10;
        if *offset + rdlength > buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns record rdata truncated",
            ));
        }
        *offset += rdlength;
    }
    Ok(())
}

fn parse_name(buf: &[u8], mut offset: usize, mut depth: u8) -> io::Result<(String, usize)> {
    if depth > 8 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "dns name compression exceeded depth",
        ));
    }

    let mut labels = Vec::new();
    let mut jumped = false;
    let mut final_offset: Option<usize> = None;

    loop {
        if offset >= buf.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "dns name extends past buffer",
            ));
        }
        let len = buf[offset];
        if len & 0xC0 == 0xC0 {
            if offset + 1 >= buf.len() {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "dns compression pointer truncated",
                ));
            }
            let pointer = (((len & 0x3F) as u16) << 8) | buf[offset + 1] as u16;
            if final_offset.is_none() {
                final_offset = Some(offset + 2);
            }
            offset = pointer as usize;
            jumped = true;
            depth += 1;
            if depth > 8 {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "dns name compression exceeded depth",
                ));
            }
            continue;
        } else if len == 0 {
            if !jumped {
                final_offset = Some(offset + 1);
            }
            break;
        } else {
            offset += 1;
            let label_len = len as usize;
            if offset + label_len > buf.len() {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "dns label extends past buffer",
                ));
            }
            let label = &buf[offset..offset + label_len];
            let text = str::from_utf8(label).map_err(|_| {
                io::Error::new(ErrorKind::InvalidData, "dns label contains invalid utf-8")
            })?;
            labels.push(text.to_string());
            offset += label_len;
            if !jumped {
                final_offset = Some(offset);
            }
        }
    }

    let consumed = final_offset.unwrap_or(offset);
    Ok((labels.join("."), consumed))
}

fn system_nameservers() -> Vec<SocketAddr> {
    let mut servers = Vec::new();

    if cfg!(unix) {
        if let Ok(contents) = fs::read_to_string("/etc/resolv.conf") {
            servers.extend(parse_resolv_conf(&contents));
        }
    }

    if cfg!(target_os = "windows") {
        if let Ok(list) = std::env::var("TB_DNS_SERVERS") {
            servers.extend(parse_env_servers(&list));
        }
    }

    if servers.is_empty() {
        servers.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            DNS_PORT,
        ));
        servers.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            DNS_PORT,
        ));
        servers.push(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
            DNS_PORT,
        ));
    }

    dedup_socket_addrs(servers)
}

fn parse_resolv_conf(contents: &str) -> Vec<SocketAddr> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') || !line.starts_with("nameserver") {
                return None;
            }
            let addr = line.split_whitespace().nth(1)?;
            parse_socket_addr(addr)
        })
        .collect()
}

fn parse_env_servers(list: &str) -> Vec<SocketAddr> {
    list.split(',')
        .filter_map(|entry| parse_socket_addr(entry.trim()))
        .collect()
}

fn parse_socket_addr(addr: &str) -> Option<SocketAddr> {
    if addr.is_empty() {
        return None;
    }

    if let Ok(parsed) = addr.parse::<SocketAddr>() {
        return Some(parsed);
    }

    if let Ok(parsed) = format!("{addr}:{DNS_PORT}").parse::<SocketAddr>() {
        return Some(parsed);
    }

    if addr.contains(':') {
        let formatted = format!("[{addr}]:{DNS_PORT}");
        if let Ok(parsed) = formatted.parse::<SocketAddr>() {
            return Some(parsed);
        }
    }

    None
}

fn dedup_socket_addrs(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for addr in addrs {
        if seen.insert(addr) {
            unique.push(addr);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_parse_name_roundtrip() {
        let mut buf = Vec::new();
        encode_name("example.com", &mut buf).unwrap();
        let (name, offset) = parse_name(&buf, 0, 0).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn parse_socket_addr_variants() {
        assert_eq!(
            parse_socket_addr("127.0.0.1"),
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                DNS_PORT
            ))
        );
        assert_eq!(
            parse_socket_addr("[2001:db8::1]:5353"),
            Some(SocketAddr::new(
                "2001:db8::1".parse::<IpAddr>().unwrap(),
                5353
            ))
        );
        assert_eq!(
            parse_socket_addr("2001:db8::1"),
            Some(SocketAddr::new(
                "2001:db8::1".parse::<IpAddr>().unwrap(),
                DNS_PORT
            ))
        );
    }

    #[test]
    fn dedupes_socket_addrs() {
        let addrs = vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DNS_PORT),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DNS_PORT),
        ];
        let deduped = dedup_socket_addrs(addrs);
        assert_eq!(deduped.len(), 1);
    }
}
