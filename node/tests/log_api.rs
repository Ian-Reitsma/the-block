#![cfg(feature = "integration-tests")]
#![allow(
    clippy::write_literal,
    clippy::manual_split_once,
    clippy::useless_format
)]
use std::fs::File;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::Path;

use sys::tempfile::tempdir;
use the_block::log_indexer::{index_logs_with_options, IndexOptions};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn run_search(path: &str) -> Result<Vec<the_block::log_indexer::LogEntry>> {
    use the_block::rpc::logs::SearchError;

    the_block::rpc::logs::run_search_for_path(path).map_err(|err| {
        let message = match err {
            SearchError::MissingDatabase => "log database unavailable".to_string(),
            SearchError::InvalidQuery(msg) => msg,
            SearchError::QueryFailed(inner) => inner.to_string(),
            SearchError::EncodeFailed(msg) => msg,
        };
        io::Error::new(io::ErrorKind::Other, message).into()
    })
}

#[test]
fn search_filters_and_decryption() -> Result<()> {
    let dir = tempdir()?;
    let log_path = dir.path().join("events.json");
    {
        let mut file = File::create(&log_path)?;
        writeln!(
            file,
            "{}",
            r#"{"timestamp":1,"level":"INFO","message":"ready","correlation_id":"alpha"}"#
        )?;
        writeln!(
            file,
            "{}",
            r#"{"timestamp":2,"level":"ERROR","message":"failed","correlation_id":"beta"}"#
        )?;
        writeln!(
            file,
            "{}",
            r#"{"timestamp":3,"level":"WARN","message":"retry","correlation_id":"beta"}"#
        )?;
        // File is automatically flushed and closed when dropped at end of scope
    }

    let db_path = dir.path().join("logs.db");
    index_logs_with_options(
        Path::new(&log_path),
        Path::new(&db_path),
        IndexOptions {
            passphrase: Some("secret".into()),
        },
    )?;

    std::env::set_var("TB_LOG_DB_PATH", db_path.to_string_lossy().to_string());
    let error_rows = run_search("/logs/search?level=ERROR&passphrase=secret")?;
    assert_eq!(error_rows.len(), 1);
    assert_eq!(error_rows[0].correlation_id, "beta");
    assert_eq!(error_rows[0].message, "failed");

    let beta_since_rows = run_search("/logs/search?correlation=beta&since=3&passphrase=secret")?;
    assert_eq!(beta_since_rows.len(), 1);
    assert_eq!(beta_since_rows[0].level, "WARN");

    let after_rows = run_search("/logs/search?after-id=1&passphrase=secret&limit=2")?;
    assert!(after_rows.iter().all(|row| row.id.unwrap_or(0) > 1));

    Ok(())
}

#[test]
fn tail_streams_indexed_rows() -> Result<()> {
    runtime::block_on(async {
        use runtime::net::{TcpListener, TcpStream};
        use runtime::ws::{self, ClientStream, Message as WsMessage};

        let dir = tempdir()?;
        let log_path = dir.path().join("events.json");
        {
            let mut file = File::create(&log_path)?;
            writeln!(
                file,
                "{}",
                r#"{"timestamp":10,"level":"INFO","message":"ready","correlation_id":"alpha"}"#
            )?;
            // File is automatically flushed and closed when dropped at end of scope
        }

        let db_path = dir.path().join("logs.db");
        index_logs_with_options(
            Path::new(&log_path),
            Path::new(&db_path),
            IndexOptions {
                passphrase: Some("secret".into()),
            },
        )?;

        // Verify the log was actually indexed before starting the server
        let mut test_filter = the_block::log_indexer::LogFilter::default();
        test_filter.passphrase = Some("secret".into());
        let indexed_entries = the_block::log_indexer::search_logs(&db_path, &test_filter)?;
        assert_eq!(indexed_entries.len(), 1, "Expected 1 indexed entry, found {}", indexed_entries.len());
        assert_eq!(indexed_entries[0].message, "ready");

        let bind_addr: SocketAddr = "127.0.0.1:0".parse()?;
        let listener = TcpListener::bind(bind_addr).await?;
        let addr = listener.local_addr()?;

        // Capture db_path for use in the server task
        let db_path_for_server = db_path.to_string_lossy().to_string();

        let server = the_block::spawn(async move {
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
            runtime::ws::write_server_handshake(&mut stream, &key, &[])
                .await
                .expect("server handshake");

            // Build tail config - it may fail if db not in query/env, but we'll override it
            let mut cfg = the_block::rpc::logs::build_tail_config(&path)
                .unwrap_or_else(|_| {
                    // If config building fails (e.g., missing DB), create a minimal config
                    // and we'll set the DB path below
                    use std::time::Duration;
                    the_block::rpc::logs::TailConfig {
                        db: std::path::PathBuf::new(),  // Placeholder, will be overridden
                        filter: the_block::log_indexer::LogFilter {
                            passphrase: Some("secret".into()),
                            ..Default::default()
                        },
                        interval: Duration::from_millis(1000),
                    }
                });

            // Override the db path to ensure we're using the correct database
            cfg.db = std::path::PathBuf::from(&db_path_for_server);

            let ws_stream = runtime::ws::ServerStream::new(stream);
            the_block::rpc::logs::run_tail(ws_stream, cfg).await;
        });

        let mut stream = TcpStream::connect(addr).await?;
        let key = ws::handshake_key();
        let path = "/logs/tail?passphrase=secret";
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n",
            host = addr
        );
        stream.write_all(request.as_bytes()).await?;
        let expected_accept = ws::handshake_accept(&key).expect("handshake accept");
        ws::read_client_handshake(&mut stream, &expected_accept).await?;
        let mut ws = ClientStream::new(stream);

        // Add timeout to prevent indefinite hang - if this times out, run_tail is not sending initial entries
        let recv_future = ws.recv();
        let timeout_duration = std::time::Duration::from_secs(3);
        let message = runtime::timeout(timeout_duration, recv_future)
            .await
            .map_err(|_| io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Timeout waiting for tail message - run_tail may not be finding entries in {:?}", db_path)
            ))??
            .expect("websocket message");

        match message {
            WsMessage::Text(text) => {
                let rows: Vec<the_block::log_indexer::LogEntry> =
                    foundation_serialization::json::from_str(&text)?;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].message, "ready");
            }
            other => panic!("unexpected message: {other:?}"),
        }
        ws.close().await?;
        drop(ws);

        // Give the server a moment to detect the closed connection and exit naturally
        runtime::sleep(std::time::Duration::from_millis(100)).await;

        // Abort the server task (which is in an infinite loop)
        // Don't await the aborted task as it may hang if the task is sleeping
        server.abort();

        Ok(())
    })
}
