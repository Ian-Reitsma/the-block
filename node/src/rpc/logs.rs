use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use httpd::StatusCode;
use runtime::ws::{Message as WsMessage, ServerStream};
use serde::Serialize;
use serde_json::json;
use url::form_urlencoded;

use crate::log_indexer::{search_logs, LogEntry, LogFilter, LogIndexerError};

#[cfg(feature = "telemetry")]
use tracing::warn;

#[derive(Debug)]
pub(crate) enum SearchError {
    MissingDatabase,
    InvalidQuery(String),
    QueryFailed(LogIndexerError),
}

pub(crate) struct TailConfig {
    db: PathBuf,
    filter: LogFilter,
    interval: Duration,
}

pub(crate) fn search_response(path: &str) -> (StatusCode, String) {
    match build_search_response(path) {
        Ok(body) => (StatusCode::OK, body),
        Err(err) => map_search_error(err),
    }
}

pub(crate) fn build_search_response(path: &str) -> Result<String, SearchError> {
    let rows = run_query(path)?;
    encode_rows(&rows)
}

fn encode_rows<T: Serialize>(rows: &T) -> Result<String, SearchError> {
    serde_json::to_string(rows)
        .map_err(LogIndexerError::from)
        .map_err(SearchError::QueryFailed)
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

pub(crate) fn build_tail_config(path: &str) -> Result<TailConfig, SearchError> {
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

pub(crate) fn map_search_error(err: SearchError) -> (StatusCode, String) {
    match err {
        SearchError::MissingDatabase => (
            StatusCode::NOT_FOUND,
            json!({"error": "log database unavailable"}).to_string(),
        ),
        SearchError::InvalidQuery(msg) => {
            (StatusCode::BAD_REQUEST, json!({"error": msg}).to_string())
        }
        SearchError::QueryFailed(err) => {
            log_query_failure(&err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
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

pub(crate) async fn run_tail(mut ws: ServerStream, cfg: TailConfig) {
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
                if let Err(err) = ws
                    .send(WsMessage::Text(
                        json!({"error": "query failed"}).to_string(),
                    ))
                    .await
                {
                    log_tail_send_failure(&err);
                }
                break;
            }
        };
        if !rows.is_empty() {
            if let Some(max_id) = rows.iter().filter_map(|row| row.id).max() {
                last_id = max_id;
            }
            let payload = match serde_json::to_string(&rows) {
                Ok(json) => json,
                Err(err) => {
                    log_serialization_failure(&err);
                    "[]".to_string()
                }
            };
            if let Err(err) = ws.send(WsMessage::Text(payload)).await {
                log_tail_send_failure(&err);
                break;
            }
        }
        runtime::sleep(cfg.interval).await;
    }
}

#[cfg(feature = "telemetry")]
fn log_serialization_failure(err: &serde_json::Error) {
    warn!(target: "rpc.logs", error = %err, "failed to serialize log tail payload");
}

#[cfg(not(feature = "telemetry"))]
fn log_serialization_failure(err: &serde_json::Error) {
    eprintln!("rpc.logs: failed to serialize log tail payload: {err}");
}

#[cfg(feature = "telemetry")]
fn log_tail_send_failure(err: &std::io::Error) {
    warn!(target: "rpc.logs", error = %err, "websocket send failed");
}

#[cfg(not(feature = "telemetry"))]
fn log_tail_send_failure(err: &std::io::Error) {
    eprintln!("rpc.logs: websocket send failed: {err}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};

    struct FailSerialize;

    impl Serialize for FailSerialize {
        fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("serialization failed"))
        }
    }

    #[test]
    fn encode_rows_reports_json_error() {
        match encode_rows(&FailSerialize) {
            Err(SearchError::QueryFailed(LogIndexerError::Json(err))) => {
                assert!(err.to_string().contains("serialization failed"));
            }
            other => panic!("unexpected encode result: {other:?}"),
        }
    }

    #[test]
    fn map_search_error_handles_json_failure() {
        let err = LogIndexerError::Json(serde_json::Error::custom("forced"));
        let (status, body) = map_search_error(SearchError::QueryFailed(err));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("log query failed"));
    }
}
