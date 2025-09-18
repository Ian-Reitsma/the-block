use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use base64::engine::general_purpose;
use base64::Engine;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{protocol::Role, Message};
use tokio_tungstenite::WebSocketStream;
use url::form_urlencoded;

use super::RpcRuntimeConfig;

use crate::log_indexer::{search_logs, LogEntry, LogFilter, LogIndexerError};

#[cfg(feature = "telemetry")]
use tracing::warn;

#[derive(Debug)]
enum SearchError {
    MissingDatabase,
    InvalidQuery(String),
    QueryFailed(LogIndexerError),
}

struct TailConfig {
    db: PathBuf,
    filter: LogFilter,
    interval: Duration,
}

pub async fn serve_search(
    mut stream: TcpStream,
    origin: &str,
    runtime_cfg: &RpcRuntimeConfig,
    path: &str,
) -> std::io::Result<()> {
    let (status, body) = match build_search_response(path) {
        Ok((status, body)) => (status, body),
        Err(err) => map_search_error(err),
    };
    let mut headers = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        status,
        body.len()
    );
    if runtime_cfg
        .cors_allow_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        headers.push_str(&format!("Access-Control-Allow-Origin: {}\r\n", origin));
    }
    headers.push_str("\r\n");
    let response = format!("{}{}", headers, body);
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

pub async fn serve_tail(mut stream: TcpStream, key: String, path: &str) {
    match build_tail_config(path) {
        Ok(cfg) => {
            if let Err(e) = handshake(&mut stream, &key).await {
                log_tail_handshake_failure(&e);
                return;
            }
            let ws_stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
            run_tail(ws_stream, cfg).await;
        }
        Err(err) => {
            let (status, body) = map_search_error(err);
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                status,
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    }
}

fn build_search_response(path: &str) -> Result<(String, String), SearchError> {
    let rows = run_query(path)?;
    let body = serde_json::to_string(&rows).map_err(|e| SearchError::QueryFailed(e.to_string()))?;
    Ok(("200 OK".into(), body))
}

fn run_query(path: &str) -> Result<Vec<LogEntry>, SearchError> {
    let (route, query) = split_route(path);
    if route != "/logs/search" {
        return Err(SearchError::InvalidQuery("unknown endpoint".into()));
    }
    let params = parse_query(query);
    let db_path = resolve_db_path(&params).ok_or(SearchError::MissingDatabase)?;
    let mut filter = build_filter(&params);
    filter.limit = params.get("limit").and_then(|v| v.parse::<usize>().ok());
    let rows = search_logs(&db_path, &filter).map_err(SearchError::QueryFailed)?;
    Ok(rows)
}

fn build_tail_config(path: &str) -> Result<TailConfig, SearchError> {
    let (route, query) = split_route(path);
    if route != "/logs/tail" {
        return Err(SearchError::InvalidQuery("unknown endpoint".into()));
    }
    let params = parse_query(query);
    let db_path = resolve_db_path(&params).ok_or(SearchError::MissingDatabase)?;
    let mut filter = build_filter(&params);
    filter.limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .or(Some(200));
    let interval = params
        .get("interval_ms")
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(1000));
    Ok(TailConfig {
        db: db_path,
        filter,
        interval,
    })
}

fn build_filter(params: &HashMap<String, String>) -> LogFilter {
    let mut filter = LogFilter::default();
    filter.peer = params.get("peer").cloned();
    filter.tx = params.get("tx").cloned();
    filter.correlation = params.get("correlation").cloned();
    filter.level = params.get("level").cloned();
    filter.passphrase = params.get("passphrase").cloned();
    filter.limit = None;
    filter.block = params.get("block").and_then(|v| v.parse::<u64>().ok());
    filter.since = params.get("since").and_then(|v| v.parse::<u64>().ok());
    filter.until = params.get("until").and_then(|v| v.parse::<u64>().ok());
    filter.after_id = params
        .get("after_id")
        .or_else(|| params.get("after-id"))
        .and_then(|v| v.parse::<u64>().ok());
    filter
}

fn resolve_db_path(params: &HashMap<String, String>) -> Option<PathBuf> {
    params
        .get("db")
        .cloned()
        .or_else(|| std::env::var("TB_LOG_DB_PATH").ok())
        .map(PathBuf::from)
}

fn parse_query(query: Option<&str>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(q) = query {
        for (key, value) in form_urlencoded::parse(q.as_bytes()) {
            map.insert(key.into_owned(), value.into_owned());
        }
    }
    map
}

fn split_route(path: &str) -> (&str, Option<&str>) {
    match path.split_once('?') {
        Some((route, query)) => (route, Some(query)),
        None => (path, None),
    }
}

fn map_search_error(err: SearchError) -> (String, String) {
    match err {
        SearchError::MissingDatabase => (
            "404 Not Found".into(),
            json!({"error": "log database unavailable"}).to_string(),
        ),
        SearchError::InvalidQuery(msg) => {
            ("400 Bad Request".into(), json!({"error": msg}).to_string())
        }
        SearchError::QueryFailed(err) => {
            log_query_failure(&err);
            (
                "500 Internal Server Error".into(),
                json!({"error": "log query failed"}).to_string(),
            )
        }
    }
}

#[cfg(feature = "telemetry")]
fn log_query_failure(err: &LogIndexerError) {
    warn!(target: "rpc.logs", error = %err, "log query failed");
}

#[cfg(not(feature = "telemetry"))]
fn log_query_failure(err: &LogIndexerError) {
    eprintln!("rpc.logs: log query failed: {err}");
}

#[cfg(feature = "telemetry")]
fn log_tail_handshake_failure(err: &std::io::Error) {
    warn!(target: "rpc.logs", "tail handshake failed: {err}");
}

#[cfg(not(feature = "telemetry"))]
fn log_tail_handshake_failure(err: &std::io::Error) {
    eprintln!("rpc.logs: tail handshake failed: {err}");
}

async fn handshake(stream: &mut TcpStream, key: &str) -> std::io::Result<()> {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept_key = general_purpose::STANDARD.encode(h.finalize());
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    );
    stream.write_all(resp.as_bytes()).await
}

async fn run_tail(mut ws: WebSocketStream<TcpStream>, cfg: TailConfig) {
    let mut last_id = cfg.filter.after_id.unwrap_or(0);
    loop {
        let mut filter = cfg.filter.clone();
        if last_id > 0 {
            filter.after_id = Some(last_id);
        }
        let rows = match search_logs(&cfg.db, &filter) {
            Ok(rows) => rows,
            Err(e) => {
                log_query_failure(&e);
                let _ = ws
                    .send(Message::Text(json!({"error": "query failed"}).to_string()))
                    .await;
                break;
            }
        };
        if !rows.is_empty() {
            if let Some(max_id) = rows.iter().filter_map(|row| row.id).max() {
                last_id = max_id;
            }
            if ws
                .send(Message::Text(
                    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into()),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
        sleep(cfg.interval).await;
    }
}

/// Convenience helper for integration tests.
pub fn run_search_for_path(path: &str) -> Result<Vec<LogEntry>, String> {
    run_query(path).map_err(|e| match e {
        SearchError::MissingDatabase => "log database unavailable".to_string(),
        SearchError::InvalidQuery(msg) => msg,
        SearchError::QueryFailed(err) => {
            log_query_failure(&err);
            "log query failed".to_string()
        }
    })
}
