#![cfg(feature = "gateway")]

use foundation_serialization::json::{self, Value as JsonValue};
use httpd::{Method, StatusCode, client::ClientResponse};
use runtime::{self, sync::mpsc};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{Arc, Mutex},
};
use the_block::{
    ReadAck, http_client, net,
    web::gateway::{self, ResolverConfig, StakeTable},
};

#[test]
fn gateway_enforces_stake_deposits() {
    runtime::block_on(async {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().expect("parse bind address");
        let listener =
            net::listener::bind_runtime("gateway", "gateway_listener_bind_failed", bind_addr)
                .await
                .expect("bind listener");
        let bound_addr = listener.local_addr().expect("listener address");
        let (read_tx, mut read_rx) = mpsc::channel::<ReadAck>(16);
        let _drain = runtime::spawn(async move { while let Some(_) = read_rx.recv().await {} });
        let stake_table = Arc::new(TestStakeTable::new());
        let stake_table_trait: Arc<dyn StakeTable + Send + Sync> = stake_table.clone();
        let server_handle = runtime::spawn(gateway::run_listener(
            listener,
            stake_table_trait,
            read_tx,
            None,
            None,
            None,
            ResolverConfig::empty(),
        ));

        let host_header = format!("example.block:{}", bound_addr.port());
        let status = send_status(bound_addr, host_header.as_str());
        assert_eq!(status, StatusCode::FORBIDDEN);

        stake_table.allow("example.block");
        let status = send_status(bound_addr, host_header.as_str());
        assert!(
            status.is_success(),
            "unexpected status after stake deposit: {status}"
        );

        server_handle.abort();
        let _ = server_handle.await;
    });
}

fn parse_status(response: &ClientResponse) -> Option<JsonValue> {
    json::from_slice::<JsonValue>(response.body()).ok()
}

#[test]
fn gateway_dns_resolver_returns_status3_until_staked() {
    runtime::block_on(async {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().expect("parse bind address");
        let listener =
            net::listener::bind_runtime("gateway", "gateway_listener_bind_failed", bind_addr)
                .await
                .expect("bind listener");
        let bound_addr = listener.local_addr().expect("listener address");
        let (read_tx, mut read_rx) = mpsc::channel::<ReadAck>(16);
        let _drain = runtime::spawn(async move { while let Some(_) = read_rx.recv().await {} });
        let stake_table = Arc::new(TestStakeTable::new());
        let stake_table_trait: Arc<dyn StakeTable + Send + Sync> = stake_table.clone();
        let server_handle = runtime::spawn(gateway::run_listener(
            listener,
            stake_table_trait,
            read_tx,
            None,
            None,
            None,
            ResolverConfig::empty(),
        ));

        let client = http_client::blocking_client();
        let url = format!(
            "http://{}/dns/resolve?name=example.block&type=A",
            bound_addr
        );
        let response = client
            .request(Method::Get, &url)
            .expect("build request")
            .send()
            .expect("send request");
        assert_eq!(response.status().as_u16(), 403);
        let body = parse_status(&response).expect("parse json");
        assert_eq!(body.get("Status").and_then(|v| v.as_u64()), Some(3));
        assert!(
            body.get("Answer")
                .and_then(|v| v.as_array())
                .map(|arr| arr.is_empty())
                .unwrap_or(false)
        );

        stake_table.allow("example.block");
        let response = client
            .request(Method::Get, &url)
            .expect("build request")
            .send()
            .expect("send request");
        assert_eq!(response.status().as_u16(), 404);
        let body = parse_status(&response).expect("parse json");
        assert_eq!(body.get("Status").and_then(|v| v.as_u64()), Some(3));
        assert!(
            body.get("Answer")
                .and_then(|v| v.as_array())
                .map(|arr| arr.is_empty())
                .unwrap_or(false)
        );

        server_handle.abort();
        let _ = server_handle.await;
    });
}

fn send_status(addr: SocketAddr, host: &str) -> StatusCode {
    let mut stream =
        TcpStream::connect(addr).expect("failed to connect to gateway listener for host check");
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        host = host
    );
    stream
        .write_all(request.as_bytes())
        .expect("failed to write request");
    stream.shutdown(Shutdown::Write).ok();
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .expect("failed to read status line");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .expect("missing status code");
    StatusCode(code)
}

#[derive(Clone)]
struct TestStakeTable {
    allowed: Arc<Mutex<HashSet<String>>>,
}

impl TestStakeTable {
    fn new() -> Self {
        Self {
            allowed: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn allow(&self, domain: &str) {
        let mut guard = self.allowed.lock().unwrap();
        guard.insert(domain.to_string());
    }
}

impl StakeTable for TestStakeTable {
    fn has_stake(&self, domain: &str) -> bool {
        let guard = self.allowed.lock().unwrap();
        guard.contains(domain)
    }
}
