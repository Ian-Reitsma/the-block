#[path = "../../tools/log_indexer.rs"]
mod log_indexer;

use log_indexer::{index_logs, search_logs, LogFilter};
use rusqlite::Connection;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn parse_and_index() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log.json");
    let mut f = File::create(&log_path).unwrap();
    writeln!(f, "{\"timestamp\":1,\"level\":\"INFO\",\"message\":\"hi\",\"correlation_id\":\"a\"}").unwrap();
    writeln!(f, "{\"timestamp\":2,\"level\":\"ERROR\",\"message\":\"bye\",\"correlation_id\":\"b\"}").unwrap();
    let db_path = dir.path().join("logs.db");
    index_logs(&log_path, &db_path).unwrap();
    let conn = Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);

    let mut filter = LogFilter::default();
    filter.correlation = Some("b".into());
    let rows = search_logs(&db_path, &filter).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message, "bye");
}
