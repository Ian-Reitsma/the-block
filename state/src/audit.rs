use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Append a law-enforcement audit trail entry to chain state.
pub fn append(base: &Path, line: &str) -> std::io::Result<()> {
    let path = base.join("le_audit.log");
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Record a storage-engine migration event in the audit log for observability.
pub fn append_engine_migration(base: &Path, from: &str, to: &str) -> std::io::Result<()> {
    let line = json!({
        "kind": "storage_engine_migration",
        "from": from,
        "to": to,
    })
    .to_string();
    append(base, &line)
}
