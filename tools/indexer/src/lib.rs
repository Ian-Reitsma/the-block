use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub hash: String,
    pub height: u64,
    pub timestamp: u64,
}

/// Simple SQLite-backed indexer.
#[derive(Clone)]
pub struct Indexer {
    path: PathBuf,
}

impl Indexer {
    /// Open or create an indexer at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let conn = Connection::open(&path_buf)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (hash TEXT PRIMARY KEY, height INTEGER, timestamp INTEGER)",
            [],
        )?;
        Ok(Self { path: path_buf })
    }

    fn conn(&self) -> Result<Connection> {
        Connection::open(&self.path)
    }

    /// Index a block record.
    pub fn index_block(&self, record: &BlockRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO blocks (hash, height, timestamp) VALUES (?1, ?2, ?3)",
            params![record.hash, record.height, record.timestamp],
        )?;
        Ok(())
    }

    /// Fetch a block by hash.
    pub fn get_block(&self, hash: &str) -> Result<BlockRecord> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT hash, height, timestamp FROM blocks WHERE hash=?1",
            params![hash],
            |row| {
                Ok(BlockRecord {
                    hash: row.get(0)?,
                    height: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            },
        )
    }

    /// Return all indexed blocks.
    pub fn all_blocks(&self) -> Result<Vec<BlockRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT hash, height, timestamp FROM blocks ORDER BY height")?;
        let rows = stmt.query_map([], |row| {
            Ok(BlockRecord {
                hash: row.get(0)?,
                height: row.get(1)?,
                timestamp: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("idx.db");
        let idx = Indexer::open(&db).unwrap();
        let rec = BlockRecord { hash: "abc".into(), height: 1, timestamp: 0 };
        idx.index_block(&rec).unwrap();
        let fetched = idx.get_block("abc").unwrap();
        assert_eq!(fetched.height, 1);
        assert_eq!(idx.all_blocks().unwrap().len(), 1);
    }
}
