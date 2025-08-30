#![cfg(feature = "telemetry")]

use std::io::{Read, Write};
use std::net::TcpStream;

use the_block::{serve_metrics_with_shutdown, telemetry};

fn init() {
    pyo3::prepare_freethreaded_python();
}

#[test]
fn metrics_http_exporter_serves_prometheus_text() {
    init();
    telemetry::MEMPOOL_SIZE
        .with_label_values(&["consumer"])
        .set(42);
    telemetry::RECORDER.tx_submitted();
    telemetry::RECORDER.tx_rejected("bad_sig");
    telemetry::RECORDER.block_mined();
    let (addr, handle) = serve_metrics_with_shutdown("127.0.0.1:0").expect("start server");
    let mut stream = TcpStream::connect(&addr).expect("connect metrics");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    assert!(buf.contains("Content-Type: text/plain"));
    assert!(buf.contains("mempool_size"));
    assert!(buf.contains("42"));
    assert!(buf.contains("tx_submitted_total 1"));
    assert!(buf.contains("tx_rejected_total{reason=\"bad_sig\"} 1"));
    assert!(buf.contains("block_mined_total 1"));
    handle.shutdown();
}
