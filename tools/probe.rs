use std::{env, fs, thread};
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
    let mut args = env::args().skip(1);
    let mut hosts = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "-f" {
            if let Some(file) = args.next() {
                if let Ok(data) = fs::read_to_string(&file) {
                    hosts.extend(data.lines().filter(|l| !l.is_empty()).map(str::to_string));
                }
            }
        } else {
            hosts.push(arg);
        }
    }
    if hosts.is_empty() {
        eprintln!("usage: probe [-f HOSTFILE] HOST[:PORT] ...");
        return;
    }
    let handles: Vec<_> = hosts
        .into_iter()
        .map(|h| {
            thread::spawn(move || {
                match fetch_metrics(&h) {
                    Ok(body) => {
                        println!("# {}", h);
                        print!("{}", body);
                    }
                    Err(err) => eprintln!("{}: {}", h, err),
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}
