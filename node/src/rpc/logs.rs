mod store {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use httpd::{form_urlencoded, StatusCode};
    use runtime::ws::{Message as WsMessage, ServerStream};
    use serde::Serialize;
    use serde_json::json;

    use crate::log_indexer::{search_logs, LogEntry, LogFilter, LogIndexerError};

    #[cfg(feature = "telemetry")]
    use tracing::warn;

    #[derive(Debug)]
    pub enum SearchError {
        MissingDatabase,
        InvalidQuery(String),
        QueryFailed(LogIndexerError),
    }

    pub struct TailConfig {
        pub db: PathBuf,
        pub filter: LogFilter,
        pub interval: Duration,
    }

    pub fn search_response(path: &str) -> (StatusCode, String) {
        match build_search_response(path) {
            Ok(body) => (StatusCode::OK, body),
            Err(err) => map_search_error(err),
        }
    }

    pub fn build_search_response(path: &str) -> Result<String, SearchError> {
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

    pub fn build_tail_config(path: &str) -> Result<TailConfig, SearchError> {
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
                map.insert(key, value);
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

    pub fn map_search_error(err: SearchError) -> (StatusCode, String) {
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

    pub async fn run_tail(mut ws: ServerStream, cfg: TailConfig) {
        let mut last_id = cfg.filter.after_id.unwrap_or(0);
        loop {
            let mut filter = cfg.filter.clone();
            if last_id > 0 {
                filter.after_id = Some(last_id);
            }
            match search_logs(&cfg.db, &filter) {
                Ok(entries) if entries.is_empty() => {}
                Ok(entries) => {
                    last_id = entries.last().and_then(|row| row.id).unwrap_or(last_id);
                    if let Ok(body) = serde_json::to_string(&entries) {
                        if ws.send(WsMessage::Text(body)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(err) => {
                    let (status, body) = map_search_error(SearchError::QueryFailed(err));
                    if ws
                        .send(WsMessage::Text(
                            json!({"status": status.as_u16(), "body": body}).to_string(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            runtime::sleep(cfg.interval).await;
        }
    }

    pub async fn serve_tail(mut stream: ServerStream, key: Vec<u8>, path: &str) {
        if key.is_empty() {
            let (status, body) = map_search_error(SearchError::MissingDatabase);
            let _ = stream
                .send(WsMessage::Text(
                    json!({"status": status.as_u16(), "body": body}).to_string(),
                ))
                .await;
            return;
        }

        match build_tail_config(path) {
            Ok(cfg) => run_tail(stream, cfg).await,
            Err(err) => {
                let (status, body) = map_search_error(err);
                let _ = stream
                    .send(WsMessage::Text(
                        json!({"status": status.as_u16(), "body": body}).to_string(),
                    ))
                    .await;
            }
        }
    }

    pub fn run_search_for_path(path: &str) -> Result<Vec<LogEntry>, SearchError> {
        run_query(path)
    }
}

pub use store::*;
