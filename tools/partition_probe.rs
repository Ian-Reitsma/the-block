use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;

fn fetch_metrics(host: &str) -> io::Result<String> {
    let trimmed = host.strip_prefix("http://").unwrap_or(host);
    let trimmed = trimmed.strip_prefix("https://").unwrap_or(trimmed);
    let (authority, _) = trimmed.split_once('/').unwrap_or((trimmed, ""));
    let addr = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let mut stream = TcpStream::connect(&addr)?;
    stream.write_all(
        format!(
            "GET /metrics HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )?;
    stream.flush()?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let separator = "\r\n\r\n";
    let body_start = response
        .find(separator)
        .map(|idx| idx + separator.len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP response"))?;
    Ok(response[body_start..].to_string())
}

fn main() {
    let host = env::args().nth(1).expect("usage: partition_probe HOST[:PORT]");
    match fetch_metrics(&host) {
        Ok(body) => {
            for line in body.lines() {
                if line.starts_with("partition_events_total")
                    || line.starts_with("partition_recover_blocks")
                {
                    println!("{}", line);
                }
            }
        }
        Err(err) => eprintln!("{}: {}", host, err),
    }
}
