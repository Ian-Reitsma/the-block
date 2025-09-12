use std::io::{Read, Write};
use std::net::SocketAddr;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use the_block::range_boost::{self, RangeBoost};

fn main() {
    // discover peers from environment
    let peers = range_boost::discover_peers();
    if let Some(best) = peers.first() {
        println!("best mesh peer {} ({} ms)", best.addr, best.latency_ms);
        if best.addr.starts_with("unix:") {
            #[cfg(unix)]
            if let Ok(mut stream) = UnixStream::connect(&best.addr[5..]) {
                let _ = stream.write_all(b"ping");
                let mut buf = [0u8; 4];
                let _ = stream.read(&mut buf);
                println!("reply {:?}", &buf);
            }
        } else if let Ok(sock) = best.addr.parse::<SocketAddr>() {
            if let Ok(mut stream) = TcpStream::connect(sock) {
                let _ = stream.write_all(b"ping");
                let mut buf = [0u8; 4];
                let _ = stream.read(&mut buf);
                println!("reply {:?}", &buf);
            }
        }
    } else {
        println!("no mesh peers found");
    }
    let mut rb = RangeBoost::new();
    rb.enqueue(b"hi".to_vec());
    println!("queued bundle, pending {}", rb.pending());
}
