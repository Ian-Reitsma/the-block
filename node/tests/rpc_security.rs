#![cfg(feature = "integration-tests")]
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use foundation_serialization::json::Value;
use runtime::{
    io::{read_to_end, BufferedTcpStream},
    net::TcpStream,
};
use std::net::SocketAddr;
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};
use util::timeout::expect_timeout;

mod util;

async fn rpc(addr: &str, body: &str, token: Option<&str>) -> Value {
    let addr: SocketAddr = addr.parse().unwrap();
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let mut req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {}\r\n", t));
    }
    req.push_str("\r\n");
    req.push_str(body);
    expect_timeout(stream.write_all(req.as_bytes()))
        .await
        .unwrap();
    let body = expect_timeout(read_http_body(stream)).await.unwrap();
    let body = String::from_utf8(body).unwrap();
    foundation_serialization::json::from_str(&body).unwrap()
}

async fn read_http_body(stream: TcpStream) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind};

    let mut reader = BufferedTcpStream::new(stream);
    let mut line = String::new();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
        return Err(Error::new(ErrorKind::UnexpectedEof, "missing status line"));
    }
    let mut content_length = None;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "truncated headers"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let len = content_length.unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body).await?;
    }
    Ok(body)
}

#[test]
fn rpc_auth_and_host_filters() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_security");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "testtoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: false,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        // host filter
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        expect_timeout(stream.write_all(
            b"POST / HTTP/1.1\r\nHost: evil.com\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        ))
        .await
        .unwrap();
        let mut buf = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut buf))
            .await
            .unwrap();
        let resp = String::from_utf8(buf).unwrap();
        assert!(resp.starts_with("HTTP/1.1 403"));

        // admin without token
        let val = rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"a","nonce":1}}"#,
            None,
        )
        .await;
        assert!(val["error"].is_object());

        // admin with token
        let val = rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"a","nonce":2}}"#,
            Some("testtoken"),
        )
        .await;
        assert_eq!(val["result"]["status"].as_str(), Some("ok"));

        handle.abort();
        let _ = handle.await;
    });
}

#[test]
fn relay_only_rejects_start_mining() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("rpc_relay_only");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let token_file = dir.path().join("token");
        std::fs::write(&token_file, "relaytoken").unwrap();
        let rpc_cfg = RpcConfig {
            admin_token_file: Some(token_file.to_str().unwrap().to_string()),
            enable_debug: true,
            relay_only: true,
            ..Default::default()
        };
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            rpc_cfg,
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        let val = rpc(
            &addr,
            r#"{"method":"start_mining","params":{"miner":"a","nonce":1}}"#,
            Some("relaytoken"),
        )
        .await;
        assert_eq!(val["result"]["error"]["code"].as_i64(), Some(-32075));
        assert_eq!(
            val["result"]["error"]["message"].as_str(),
            Some("relay_only")
        );
        assert!(!mining.load(std::sync::atomic::Ordering::SeqCst));

        handle.abort();
        let _ = handle.await;
    });
}
