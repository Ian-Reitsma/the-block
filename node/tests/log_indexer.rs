#![cfg(feature = "integration-tests")]
use sled;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;
use the_block::log_indexer::{
    index_logs, index_logs_with_options, search_logs, IndexOptions, LogFilter, LogIndexerError,
};

fn entry_key(id: u64) -> String {
    format!("entry:{id:016x}")
}

#[test]
fn parse_and_index() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log.json");
    let mut f = File::create(&log_path).unwrap();
    writeln!(
        f,
        "{}",
        r#"{"timestamp":1,"level":"INFO","message":"hi","correlation_id":"a"}"#
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        r#"{"timestamp":2,"level":"ERROR","message":"bye","correlation_id":"b"}"#
    )
    .unwrap();
    let db_path = dir.path().join("logs.db");
    index_logs(&log_path, &db_path).unwrap();

    let rows = search_logs(&db_path, &LogFilter::default()).unwrap();
    assert_eq!(rows.len(), 2);

    let mut filter = LogFilter::default();
    filter.correlation = Some("b".into());
    let rows = search_logs(&db_path, &filter).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message, "bye");
}

#[test]
fn surfaces_decryption_errors() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("logs.db");
    let log_path = dir.path().join("events.json");
    let mut file = File::create(&log_path).unwrap();
    writeln!(
        file,
        "{}",
        r#"{"timestamp":1,"level":"INFO","message":"hello","correlation_id":"x"}"#
    )
    .unwrap();
    index_logs(&log_path, &db_path).unwrap();

    let db = sled::open(&db_path).unwrap();
    let tree = db.open_tree("entries").unwrap();
    tree.insert(entry_key(1), b"not-json").unwrap();

    let err = search_logs(&db_path, &LogFilter::default()).unwrap_err();
    match err {
        LogIndexerError::Json(_) => {}
        other => panic!("unexpected error variant: {:?}", other),
    }
}

#[test]
fn encrypts_and_decrypts_round_trip() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("secure.json");
    let mut f = File::create(&log_path).unwrap();
    writeln!(
        f,
        "{}",
        r#"{"timestamp":5,"level":"INFO","message":"secret","correlation_id":"c"}"#
    )
    .unwrap();
    let db_path = dir.path().join("logs.db");
    index_logs_with_options(
        &log_path,
        &db_path,
        IndexOptions {
            passphrase: Some("hunter2".into()),
        },
    )
    .unwrap();

    let mut filter = LogFilter::default();
    let rows = search_logs(&db_path, &filter).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message, "<encrypted>");

    filter.passphrase = Some("hunter2".into());
    let decrypted = search_logs(&db_path, &filter).unwrap();
    assert_eq!(decrypted.len(), 1);
    assert_eq!(decrypted[0].message, "secret");
}
