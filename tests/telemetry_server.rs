#![cfg(feature = "telemetry")]

use std::io::{Read, Write};
use std::net::TcpStream;

use the_block::{serve_metrics, telemetry};

fn init() {
    pyo3::prepare_freethreaded_python();
}

#[test]
fn metrics_http_exporter_serves_prometheus_text() {
    init();
    telemetry::MEMPOOL_SIZE.set(42);
    let addr = serve_metrics("127.0.0.1:0").expect("start server");
    let mut stream = TcpStream::connect(addr).expect("connect metrics");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    assert!(buf.contains("mempool_size"));
    assert!(buf.contains("42"));
}
