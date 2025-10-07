use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;

/// Record describing a detected network partition.
pub struct PartitionRecord {
    pub timestamp: u64,
    pub peers: Vec<SocketAddr>,
    pub notes: String,
}

/// Append a partition record to persistent log at `path`.
pub fn append(record: &PartitionRecord, path: &Path) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let bytes = record.encode();
    file.write_all(&bytes)?;
    Ok(())
}

impl PartitionRecord {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.extend_from_slice(&(self.peers.len() as u32).to_be_bytes());
        for peer in &self.peers {
            let encoded = peer.to_string();
            let bytes = encoded.as_bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(bytes);
        }
        let note_bytes = self.notes.as_bytes();
        out.extend_from_slice(&(note_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(note_bytes);
        out
    }
}
