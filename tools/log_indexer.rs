use std::fs::File;
use std::path::Path;

use log_index::{
    ingest_with_seek_and_observer, search_logs_in_store, IndexOptions, LogEntry, LogFilter,
    LogIndexError, LogStore, StoredEntry,
};

pub type LogIndexerError = LogIndexError;
pub type Result<T, E = LogIndexerError> = std::result::Result<T, E>;

/// Index JSON log lines with explicit options such as encryption.
pub fn index_logs_with_options(log_path: &Path, db_path: &Path, opts: IndexOptions) -> Result<()> {
    let store = LogStore::open(db_path)?;
    let mut file = File::open(log_path)?;
    let source = canonical_source_key(log_path);
    ingest_with_seek_and_observer(&mut file, &source, &opts, &store, |entry: &StoredEntry| {
        increment_indexed_metric(&entry.correlation_id)
    })
}

fn canonical_source_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

// Telemetry removed - module doesn't exist in log-indexer-cli
#[cfg(feature = "telemetry")]
fn increment_indexed_metric(_correlation_id: &str) {
    // TODO: wire telemetry when module exists
}

#[cfg(not(feature = "telemetry"))]
fn increment_indexed_metric(_correlation_id: &str) {}

/// Search indexed logs with optional filters.
pub fn search_logs(db_path: &Path, filter: &LogFilter) -> Result<Vec<LogEntry>> {
    let store = LogStore::open(db_path)?;
    let results = search_logs_in_store(&store, filter)?;

    // Telemetry removed - module doesn't exist
    #[cfg(feature = "telemetry")]
    {
        if filter
            .correlation
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            && results.is_empty()
        {
            // TODO: wire telemetry when module exists
            // crate::telemetry::record_log_correlation_fail();
        }
    }

    Ok(results)
}
