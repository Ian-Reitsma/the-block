mod store {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use foundation_serialization::{
        json::{self, Map, Number, Value},
        Serialize,
    };
    use httpd::{form_urlencoded, StatusCode};
    use runtime::ws::{Message as WsMessage, ServerStream};

    use crate::log_indexer::{search_logs, LogEntry, LogFilter, LogIndexerError};

    #[cfg(feature = "telemetry")]
    use diagnostics::tracing::warn;

    #[derive(Debug)]
    pub enum SearchError {
        MissingDatabase,
        InvalidQuery(String),
        QueryFailed(LogIndexerError),
        EncodeFailed(String),
    }

    pub struct TailConfig {
        pub db: PathBuf,
        pub filter: LogFilter,
        pub interval: Duration,
    }

    fn error_value(message: impl Into<String>) -> Value {
        let mut map = Map::new();
        map.insert("error".to_string(), Value::String(message.into()));
        Value::Object(map)
    }

    fn status_body_value(status: StatusCode, body: String) -> Value {
        let mut map = Map::new();
        map.insert(
            "status".to_string(),
            Value::Number(Number::from(status.as_u16())),
        );
        map.insert("body".to_string(), Value::String(body));
        Value::Object(map)
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
        json::to_string(rows).map_err(|err| SearchError::EncodeFailed(err.to_string()))
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
                json::to_string_value(&error_value("log database unavailable")),
            ),
            SearchError::InvalidQuery(msg) => (
                StatusCode::BAD_REQUEST,
                json::to_string_value(&error_value(msg)),
            ),
            SearchError::QueryFailed(err) => {
                log_query_failure(&err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json::to_string_value(&error_value("log query failed")),
                )
            }
            SearchError::EncodeFailed(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json::to_string_value(&error_value(format!("serialization failed: {err}"))),
            ),
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
                    if let Ok(body) = json::to_string(&entries) {
                        if ws.send(WsMessage::Text(body)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(err) => {
                    let (status, body) = map_search_error(SearchError::QueryFailed(err));
                    let response = json::to_string_value(&status_body_value(status, body));
                    if ws.send(WsMessage::Text(response)).await.is_err() {
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
            let payload = json::to_string_value(&status_body_value(status, body));
            let _ = stream.send(WsMessage::Text(payload)).await;
            return;
        }

        match build_tail_config(path) {
            Ok(cfg) => run_tail(stream, cfg).await,
            Err(err) => {
                let (status, body) = map_search_error(err);
                let payload = json::to_string_value(&status_body_value(status, body));
                let _ = stream.send(WsMessage::Text(payload)).await;
            }
        }
    }

    pub fn run_search_for_path(path: &str) -> Result<Vec<LogEntry>, SearchError> {
        run_query(path)
    }
}

pub use store::*;
