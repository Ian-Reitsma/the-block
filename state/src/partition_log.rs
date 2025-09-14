use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;

/// Record describing a detected network partition.
#[derive(Serialize, Deserialize)]
pub struct PartitionRecord {
    pub timestamp: u64,
    pub peers: Vec<SocketAddr>,
    pub notes: String,
}

/// Append a partition record to persistent log at `path`.
pub fn append(record: &PartitionRecord, path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let bytes = bincode::serialize(record).expect("serialize partition record");
    file.write_all(&bytes)?;
    Ok(())
}
