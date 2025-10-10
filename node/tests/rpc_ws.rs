#![cfg(feature = "integration-tests")]

use std::sync::{atomic::AtomicBool, Arc, Mutex, Once};

use diagnostics::anyhow::Result;
use runtime::io::{AsyncReadExt, AsyncWriteExt};
use runtime::net::TcpStream;
use runtime::sync::oneshot;
use runtime::ws;
use the_block::config::RpcConfig;
use the_block::rpc::run_rpc_server;
use the_block::Blockchain;

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

async fn read_response_headers(stream: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 256];
    loop {
        let read = stream.read(&mut tmp).await?;
        if read == 0 {
            diagnostics::anyhow::bail!("connection closed before headers");
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = &buf[..pos + 4];
            return Ok(String::from_utf8(header.to_vec())?);
        }
    }
}

async fn spawn_rpc_server() -> (
    std::net::SocketAddr,
    runtime::JoinHandle<()>,
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

#[test]
fn state_stream_upgrade_requires_headers() -> Result<()> {
    runtime::block_on(async {
        configure_runtime();
        let (addr, server, _dir) = spawn_rpc_server().await;

        let mut client = TcpStream::connect(addr).await?;
        let key = ws::handshake_key();
        let request = format!(
        "GET /state_stream HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
        client.write_all(request.as_bytes()).await?;
        let headers = read_response_headers(&mut client).await?;
        assert!(
            headers.starts_with("HTTP/1.1 101"),
            "unexpected response: {headers}"
        );
        client.shutdown().await?;
        server.abort();
        let _ = server.await;
        Ok(())
    })
}

#[test]
fn missing_upgrade_header_is_rejected() -> Result<()> {
    runtime::block_on(async {
        configure_runtime();
        let (addr, server, _dir) = spawn_rpc_server().await;

        let mut client = TcpStream::connect(addr).await?;
        let key = ws::handshake_key();
        let request = format!(
        "GET /state_stream HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
        client.write_all(request.as_bytes()).await?;
        let headers = read_response_headers(&mut client).await?;
        assert!(
            headers.starts_with("HTTP/1.1 400"),
            "unexpected response: {headers}"
        );
        client.shutdown().await?;
        server.abort();
        let _ = server.await;
        Ok(())
    })
}
