use std::collections::HashSet;
use std::sync::{atomic::AtomicBool, Arc, Mutex, Once};
use std::time::Duration;

use anyhow::Result;
use runtime::net::{TcpListener, TcpStream};
use runtime::ws;
use the_block::identity::did::DidRegistry;
use the_block::identity::handle_registry::HandleRegistry;
use the_block::rpc::{self, RpcRuntimeConfig};

fn configure_runtime() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("TB_RUNTIME_BACKEND", "tokio");
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
            anyhow::bail!("connection closed before headers");
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = &buf[..pos + 4];
            return Ok(String::from_utf8(header.to_vec())?);
        }
    }
}

fn runtime_config() -> Arc<RpcRuntimeConfig> {
    Arc::new(RpcRuntimeConfig {
        allowed_hosts: vec!["localhost".into()],
        cors_allow_origins: Vec::new(),
        max_body_bytes: 1024,
        request_timeout: Duration::from_secs(1),
        enable_debug: true,
        admin_token: None,
        relay_only: false,
    })
}

fn registries() -> (Arc<Mutex<HandleRegistry>>, Arc<Mutex<DidRegistry>>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.into_path();
    let handle_path = base.join("handles");
    let did_path = base.join("dids");
    let handles = HandleRegistry::open(handle_path.to_string_lossy().as_ref());
    let dids = DidRegistry::open(&did_path);
    (Arc::new(Mutex::new(handles)), Arc::new(Mutex::new(dids)))
}

fn spawn_rpc_handler(listener: TcpListener) -> runtime::JoinHandle<()> {
    let bc = Arc::new(Mutex::new(the_block::Blockchain::default()));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::<(String, u64)>::new()));
    let (handles, dids) = registries();
    let cfg = runtime_config();
    the_block::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            rpc::handle_conn(stream, bc, mining, nonces, handles, dids, cfg).await;
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_stream_upgrade_requires_headers() -> Result<()> {
    configure_runtime();
    let listener = TcpListener::bind("127.0.0.1:0".parse()?).await?;
    let addr = listener.local_addr()?;
    let server = spawn_rpc_handler(listener);

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
    server.await.unwrap();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_upgrade_header_is_rejected() -> Result<()> {
    configure_runtime();
    let listener = TcpListener::bind("127.0.0.1:0".parse()?).await?;
    let addr = listener.local_addr()?;
    let server = spawn_rpc_handler(listener);

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
    server.await.unwrap();
    Ok(())
}
