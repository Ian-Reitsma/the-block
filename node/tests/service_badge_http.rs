#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use foundation_serialization::json::Value;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use the_block::{config::RpcConfig, rpc::run_rpc_server, Blockchain};

use runtime::{
    io::BufferedTcpStream,
    net::TcpStream,
};
use std::net::SocketAddr;
use util::timeout::expect_timeout;

mod util;

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
fn badge_status_endpoint() {
    runtime::block_on(async {
        let dir = util::temp::temp_dir("badge_status");
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".into(),
            RpcConfig::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        // Initially no badge should be active.
        let addr_socket: SocketAddr = addr.parse().unwrap();
        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        expect_timeout(stream.write_all(
            b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        ))
        .await
        .unwrap();
        let body = expect_timeout(read_http_body(stream)).await.unwrap();
        let body: Value = foundation_serialization::json::from_slice(&body).unwrap();
        assert!(!body["active"].as_bool().unwrap());
        assert!(matches!(body.get("last_mint"), Some(Value::Null)));
        assert!(matches!(body.get("last_burn"), Some(Value::Null)));

        // Mint a badge and verify the endpoint reflects it.
        {
            let mut chain = bc.lock().unwrap();
            for _ in 0..90 {
                chain.badge_tracker_mut().record_epoch(
                    "node",
                    true,
                    std::time::Duration::from_millis(0),
                );
            }
        }

        let mut stream = expect_timeout(TcpStream::connect(addr_socket))
            .await
            .unwrap();
        expect_timeout(stream.write_all(
            b"GET /badge/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        ))
        .await
        .unwrap();
        let body = expect_timeout(read_http_body(stream)).await.unwrap();
        let body: Value = foundation_serialization::json::from_slice(&body).unwrap();
        assert!(body["active"].as_bool().unwrap());
        assert!(body["last_mint"].as_u64().is_some());

        handle.abort();
        let _ = handle.await;
    });
}
