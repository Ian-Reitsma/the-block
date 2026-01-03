#![cfg(feature = "integration-tests")]

use std::sync::{atomic::AtomicBool, Arc, Mutex, Once};
use std::time::Duration;

use concurrency::Lazy;
use diagnostics::anyhow::Result;
use runtime::net::TcpStream;
use runtime::sync::oneshot;
use runtime::ws;
use the_block::config::RpcConfig;
use the_block::rpc::run_rpc_server;
use the_block::Blockchain;

struct RpcServerState {
    addr: std::net::SocketAddr,
    _handle: runtime::JoinHandle<Result<(), std::io::Error>>,
    _dir: sys::tempfile::TempDir,
}

static RPC_SERVER: Lazy<Mutex<Option<RpcServerState>>> = Lazy::new(|| Mutex::new(None));

fn configure_runtime() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "inhouse");
        the_block::simple_db::configure_engines(the_block::simple_db::EngineConfig {
            default_engine: the_block::simple_db::EngineKind::Memory,
            overrides: Default::default(),
        });
    });
}

async fn expect_timeout_with<F, T, E>(fut: F, context: &str) -> Result<T>
where
    F: std::future::Future<Output = std::result::Result<T, E>>,
    E: Into<diagnostics::anyhow::Error>,
{
    let fut = async { fut.await.map_err(Into::into) };
    the_block::timeout(Duration::from_secs(30), fut)
        .await
        .map_err(|_| diagnostics::anyhow::anyhow!("operation timed out: {context}"))?
}

async fn read_response_headers(stream: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 256];
    loop {
        let read = stream.read(&mut tmp).await.expect("read headers");
        if read == 0 {
            diagnostics::anyhow::bail!("connection closed before headers");
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = &buf[..pos + 4];
            return Ok(String::from_utf8(header.to_vec()).expect("utf8 headers"));
        }
    }
}

async fn spawn_rpc_server() -> (
    std::net::SocketAddr,
    runtime::JoinHandle<Result<(), std::io::Error>>,
    sys::tempfile::TempDir,
) {
    let dir = sys::tempfile::tempdir().expect("tempdir");
    let chain_path = dir.path().join("chain");
    let bc = Arc::new(Mutex::new(Blockchain::new(
        chain_path.to_string_lossy().as_ref(),
    )));
    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = oneshot::channel();
    let rpc_cfg = RpcConfig {
        enable_debug: true,
        relay_only: false,
        ..Default::default()
    };
    let handle = the_block::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".into(),
        rpc_cfg,
        tx,
    ));
    let addr = rx.await.expect("rpc ready");
    let socket = addr.parse().expect("socket address");
    (socket, handle, dir)
}

async fn rpc_server_addr() -> Result<std::net::SocketAddr> {
    configure_runtime();
    if let Some(addr) = RPC_SERVER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|state| state.addr)
    {
        return Ok(addr);
    }

    let (addr, handle, dir) = spawn_rpc_server().await;
    let mut guard = RPC_SERVER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing_addr) = guard.as_ref().map(|state| state.addr) {
        drop(guard);
        handle.abort();
        let _ = handle.await;
        return Ok(existing_addr);
    }
    *guard = Some(RpcServerState {
        addr,
        _handle: handle,
        _dir: dir,
    });
    Ok(addr)
}

#[testkit::tb_serial]
fn state_stream_upgrade_requires_headers() -> Result<()> {
    configure_runtime();
    runtime::block_on(async {
        let addr = rpc_server_addr().await?;

        let mut client = expect_timeout_with(TcpStream::connect(addr), "connect").await?;
        let key = ws::handshake_key();
        let request = format!(
        "GET /state_stream HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
        expect_timeout_with(client.write_all(request.as_bytes()), "write")
            .await
            .expect("write handshake");
        let headers = expect_timeout_with(read_response_headers(&mut client), "read headers")
            .await?;
        assert!(
            headers.starts_with("HTTP/1.1 101"),
            "unexpected response: {headers}"
        );
        if let Err(err) = client.shutdown().await {
            if err.kind() != std::io::ErrorKind::NotConnected {
                return Err(err.into());
            }
        }
        Ok(())
    })
}

#[testkit::tb_serial]
fn missing_upgrade_header_is_rejected() -> Result<()> {
    configure_runtime();
    runtime::block_on(async {
        let addr = rpc_server_addr().await?;

        let mut client = expect_timeout_with(TcpStream::connect(addr), "connect").await?;
        let key = ws::handshake_key();
        let request = format!(
        "GET /state_stream HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
        expect_timeout_with(client.write_all(request.as_bytes()), "write")
            .await
            .expect("write handshake");
        let headers = expect_timeout_with(read_response_headers(&mut client), "read headers")
            .await?;
        assert!(
            headers.starts_with("HTTP/1.1 400"),
            "unexpected response: {headers}"
        );
        if let Err(err) = client.shutdown().await {
            if err.kind() != std::io::ErrorKind::NotConnected {
                return Err(err.into());
            }
        }
        Ok(())
    })
}
