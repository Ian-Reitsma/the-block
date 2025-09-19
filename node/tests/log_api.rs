#![cfg(feature = "integration-tests")]
use std::fs::File;
use std::io::Write;
use std::path::Path;

use tempfile::tempdir;
use the_block::log_indexer::{index_logs_with_options, IndexOptions};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn search_filters_and_decryption() -> Result<()> {
    let dir = tempdir()?;
    let log_path = dir.path().join("events.json");
    let mut file = File::create(&log_path)?;
    writeln!(
        file,
        "{{\"timestamp\":1,\"level\":\"INFO\",\"message\":\"ready\",\"correlation_id\":\"alpha\"}}"
    )?;
    writeln!(file, "{{\"timestamp\":2,\"level\":\"ERROR\",\"message\":\"failed\",\"correlation_id\":\"beta\"}}")?;
    writeln!(
        file,
        "{{\"timestamp\":3,\"level\":\"WARN\",\"message\":\"retry\",\"correlation_id\":\"beta\"}}"
    )?;

    let db_path = dir.path().join("logs.db");
    index_logs_with_options(
        Path::new(&log_path),
        Path::new(&db_path),
        IndexOptions {
            passphrase: Some("secret".into()),
        },
    )?;

    std::env::set_var("TB_LOG_DB_PATH", db_path.to_string_lossy().to_string());
    let error_rows =
        the_block::rpc::logs::run_search_for_path("/logs/search?level=ERROR&passphrase=secret")?;
    assert_eq!(error_rows.len(), 1);
    assert_eq!(error_rows[0].correlation_id, "beta");
    assert_eq!(error_rows[0].message, "failed");

    let beta_since_rows = the_block::rpc::logs::run_search_for_path(
        "/logs/search?correlation=beta&since=3&passphrase=secret",
    )?;
    assert_eq!(beta_since_rows.len(), 1);
    assert_eq!(beta_since_rows[0].level, "WARN");

    let after_rows = the_block::rpc::logs::run_search_for_path(
        "/logs/search?after-id=1&passphrase=secret&limit=2",
    )?;
    assert!(after_rows.iter().all(|row| row.id.unwrap_or(0) > 1));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tail_streams_indexed_rows() -> Result<()> {
    use futures::StreamExt;
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{client_async, tungstenite::Message};

    let dir = tempdir()?;
    let log_path = dir.path().join("events.json");
    let mut file = File::create(&log_path)?;
    writeln!(
        file,
        "{\"timestamp\":10,\"level\":\"INFO\",\"message\":\"ready\",\"correlation_id\":\"alpha\"}"
    )?;

    let db_path = dir.path().join("logs.db");
    index_logs_with_options(
        Path::new(&log_path),
        Path::new(&db_path),
        IndexOptions {
            passphrase: Some("secret".into()),
        },
    )?;

    std::env::set_var("TB_LOG_DB_PATH", db_path.to_string_lossy().to_string());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let mut request_bytes = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let read = stream
                .read(&mut buf)
                .await
                .expect("read websocket handshake");
            if read == 0 {
                break;
            }
            request_bytes.extend_from_slice(&buf[..read]);
            if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(request_bytes).expect("handshake utf8");
        let path = request
            .lines()
            .find_map(|line| {
                if line.starts_with("GET ") {
                    line.split_whitespace().nth(1).map(str::to_string)
                } else {
                    None
                }
            })
            .expect("request path");
        let key = request
            .lines()
            .find_map(|line| {
                if line.to_ascii_lowercase().starts_with("sec-websocket-key:") {
                    line.splitn(2, ':').nth(1).map(|v| v.trim().to_string())
                } else {
                    None
                }
            })
            .expect("websocket key");
        the_block::rpc::logs::serve_tail(stream, key, &path).await;
        Ok::<(), Box<dyn std::error::Error>>(())
    });

    let stream = TcpStream::connect(addr).await?;
    let url = format!("ws://{}/logs/tail?passphrase=secret", addr);
    let (mut ws, _) = client_async(url, stream).await?;
    let message = ws
        .next()
        .await
        .expect("websocket message")
        .expect("message result");
    match message {
        Message::Text(text) => {
            let rows: Vec<the_block::log_indexer::LogEntry> = serde_json::from_str(&text)?;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].message, "ready");
        }
        other => panic!("unexpected message: {other:?}"),
    }
    ws.close(None).await?;
    drop(ws);
    server.await.expect("server task")?;
    std::env::remove_var("TB_LOG_DB_PATH");
    Ok(())
}
