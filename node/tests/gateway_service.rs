#![cfg(feature = "gateway")]

use foundation_serialization::json::{self, Value as JsonValue};
use httpd::{client::ClientResponse, Method, StatusCode};
use runtime::{self, sync::mpsc};
use std::{
    collections::HashSet,
    env,
    io::{BufRead, BufReader, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{Arc, Mutex},
};
use sys::tempfile::tempdir;
use the_block::{
    drive, http_client, net,
    storage::pipeline,
    web::gateway::{self, ResolverConfig, StakeTable},
    ReadAck,
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

async fn start_gateway_server(
    resolver: ResolverConfig,
) -> (SocketAddr, runtime::JoinHandle<()>, Arc<TestStakeTable>) {
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
    let server_handle = runtime::spawn({
        let stake_table_trait = stake_table_trait.clone();
        let resolver = resolver.clone();
        async move {
            let _ = gateway::run_listener(
                listener,
                stake_table_trait,
                read_tx,
                None,
                None,
                None,
                resolver,
            )
            .await;
        }
    });
    (bound_addr, server_handle, stake_table)
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
        assert!(body
            .get("Answer")
            .and_then(|v| v.as_array())
            .map(|arr| arr.is_empty())
            .unwrap_or(false));

        stake_table.allow("example.block");
        let response = client
            .request(Method::Get, &url)
            .expect("build request")
            .send()
            .expect("send request");
        assert_eq!(response.status().as_u16(), 404);
        let body = parse_status(&response).expect("parse json");
        assert_eq!(body.get("Status").and_then(|v| v.as_u64()), Some(3));
        assert!(body
            .get("Answer")
            .and_then(|v| v.as_array())
            .map(|arr| arr.is_empty())
            .unwrap_or(false));

        server_handle.abort();
        let _ = server_handle.await;
    });
}

#[test]
fn gateway_dns_resolver_returns_successful_answers() {
    runtime::block_on(async {
        let resolver = ResolverConfig::with_addresses(vec!["1.2.3.4".parse().unwrap()], 37, None);
        let (bind_addr, server_handle, stake_table) = start_gateway_server(resolver).await;
        stake_table.allow("example.block");
        let host_header = format!("example.block:{}", bind_addr.port());
        let client = http_client::blocking_client();
        let url = format!("http://{}/dns/resolve?name=example.block&type=A", bind_addr);
        let response = client
            .request(Method::Get, &url)
            .expect("build request")
            .header("Host", host_header.as_str())
            .send()
            .expect("send request");
        assert_eq!(response.status().as_u16(), 200);
        let body: JsonValue = json::from_slice(response.body()).expect("parse json");
        assert_eq!(body.get("Status").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(response.header("cache-control"), Some("max-age=37"));
        let answers = body
            .get("Answer")
            .and_then(|value| value.as_array())
            .expect("missing answers");
        let first = answers[0].as_object().expect("answer object");
        assert_eq!(
            first.get("data").and_then(|value| value.as_str()),
            Some("1.2.3.4")
        );
        server_handle.abort();
        let _ = server_handle.await;
    });
}

#[test]
fn gateway_drive_and_static_routes_serve_content() {
    runtime::block_on(async {
        let _pipeline_guard = pipeline::PipelineTestGuard::new();
        pipeline::override_static_blob_for_test("example.block", "/", b"static-root".to_vec());
        let tmp_dir = tempdir().expect("create tempdir");
        let drive_base = tmp_dir.path().join("drive");
        let _drive_env = EnvVarGuard::set(
            "TB_DRIVE_BASE_DIR",
            drive_base.to_str().expect("drive path"),
        );
        let drive_store = drive::DriveStore::with_base(drive_base.clone());
        let object_id = drive_store
            .store(b"drive-secret")
            .expect("store drive object");
        let (bind_addr, server_handle, stake_table) =
            start_gateway_server(ResolverConfig::empty()).await;
        stake_table.allow("example.block");
        let host_header = format!("example.block:{}", bind_addr.port());
        let client = http_client::blocking_client();
        let drive_url = format!("http://{}/drive/{}", bind_addr, object_id);
        let drive_response = client
            .request(Method::Get, &drive_url)
            .expect("build drive request")
            .header("Host", host_header.as_str())
            .send()
            .expect("send drive request");
        assert_eq!(drive_response.status().as_u16(), 200);
        assert_eq!(
            drive_response.header("content-type"),
            Some("application/octet-stream")
        );
        assert_eq!(drive_response.body(), b"drive-secret");

        let static_url = format!("http://{}/", bind_addr);
        let static_client = http_client::blocking_client();
        let static_response = static_client
            .request(Method::Get, &static_url)
            .expect("build static request")
            .header("Host", host_header.as_str())
            .send()
            .expect("send static request");
        assert_eq!(static_response.status().as_u16(), 200);
        assert_eq!(static_response.body(), b"static-root");
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

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var(key).ok();
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            env::set_var(self.key, previous);
        } else {
            env::remove_var(self.key);
        }
    }
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
