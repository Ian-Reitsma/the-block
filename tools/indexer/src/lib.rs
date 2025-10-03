use httpd::{HttpError, Request, Response, Router, StatusCode};
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use hex::encode as hex_encode;
use the_block::compute_market::receipt::Receipt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub hash: String,
    pub height: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRecord {
    pub key: String,
    pub epoch: u64,
    pub provider: String,
    pub buyer: String,
    pub amount: u64,
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
        conn.execute(
            "CREATE TABLE IF NOT EXISTS receipts (key TEXT PRIMARY KEY, epoch INTEGER, provider TEXT, buyer TEXT, amount INTEGER)",
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

    /// Index a receipt record.
    pub fn index_receipt(&self, record: &ReceiptRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO receipts (key, epoch, provider, buyer, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.key, record.epoch, record.provider, record.buyer, record.amount],
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
        let mut stmt =
            conn.prepare("SELECT hash, height, timestamp FROM blocks ORDER BY height")?;
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

    /// Return all indexed receipts.
    pub fn all_receipts(&self) -> Result<Vec<ReceiptRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT key, epoch, provider, buyer, amount FROM receipts ORDER BY epoch")?;
        let rows = stmt.query_map([], |row| {
            Ok(ReceiptRecord {
                key: row.get(0)?,
                epoch: row.get(1)?,
                provider: row.get(2)?,
                buyer: row.get(3)?,
                amount: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Helper to index all receipts in a pending directory.
    pub fn index_receipts_dir(&self, dir: &Path) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for ent in entries.flatten() {
                if let Ok(epoch) = ent.file_name().to_string_lossy().parse::<u64>() {
                    if let Ok(bytes) = std::fs::read(ent.path()) {
                        if let Ok(list) = bincode::deserialize::<Vec<Receipt>>(&bytes) {
                            for r in list {
                                let rec = ReceiptRecord {
                                    key: hex_encode(r.idempotency_key),
                                    epoch,
                                    provider: r.provider.clone(),
                                    buyer: r.buyer.clone(),
                                    amount: r.quote_price,
                                };
                                let _ = self.index_receipt(&rec);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn router(state: Indexer) -> Router<Indexer> {
    Router::new(state).get("/blocks", list_blocks)
}

async fn list_blocks(request: Request<Indexer>) -> Result<Response, HttpError> {
    let indexer = request.state().clone();
    let blocks = indexer
        .all_blocks()
        .map_err(|err| HttpError::Handler(err.to_string()))?;
    Response::new(StatusCode::OK).json(&blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpd::StatusCode;

    #[test]
    fn index_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("idx.db");
        let idx = Indexer::open(&db).unwrap();
        let rec = BlockRecord {
            hash: "abc".into(),
            height: 1,
            timestamp: 0,
        };
        idx.index_block(&rec).unwrap();
        let fetched = idx.get_block("abc").unwrap();
        assert_eq!(fetched.height, 1);
        assert_eq!(idx.all_blocks().unwrap().len(), 1);
    }

    #[test]
    fn index_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("idx.db");
        let idx = Indexer::open(&db).unwrap();
        let rec = ReceiptRecord {
            key: "k".into(),
            epoch: 1,
            provider: "p".into(),
            buyer: "b".into(),
            amount: 5,
        };
        idx.index_receipt(&rec).unwrap();
        assert_eq!(idx.all_receipts().unwrap().len(), 1);
    }

    #[test]
    fn router_lists_blocks() {
        runtime::block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("idx.db");
            let idx = Indexer::open(&db).unwrap();
            let rec = BlockRecord {
                hash: "abc".into(),
                height: 1,
                timestamp: 42,
            };
            idx.index_block(&rec).unwrap();

            let app = router(idx.clone());
            let response = app
                .handle(app.request_builder().path("/blocks").build())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let blocks: Vec<BlockRecord> = serde_json::from_slice(response.body()).unwrap();
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].hash, "abc");
        });
    }
}
