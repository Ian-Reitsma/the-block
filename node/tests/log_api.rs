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
