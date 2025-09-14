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
