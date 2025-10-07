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
    let line = format!(
        "{{\"kind\":\"storage_engine_migration\",\"from\":\"{}\",\"to\":\"{}\"}}",
        escape_json(from),
        escape_json(to)
    );
    append(base, &line)
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(&mut escaped, "\\u{:04x}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}
