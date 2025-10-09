use foundation_serialization::{
    binary,
    json::{self, Map, Number, Value},
};
use httpd::{HttpError, Request, Response, Router, StatusCode};
use rusqlite::{params, Connection, Result};
use std::path::{Path, PathBuf};

use the_block::compute_market::receipt::Receipt;

#[derive(Debug, Clone)]
pub struct BlockRecord {
    pub hash: String,
    pub height: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct ReceiptRecord {
    pub key: String,
    pub epoch: u64,
    pub provider: String,
    pub buyer: String,
    pub amount: u64,
}

impl BlockRecord {
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("hash".to_string(), Value::String(self.hash.clone()));
        map.insert(
            "height".to_string(),
            Value::Number(Number::from(self.height)),
        );
        map.insert(
            "timestamp".to_string(),
            Value::Number(Number::from(self.timestamp)),
        );
        Value::Object(map)
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        let map = value
            .as_object()
            .ok_or_else(|| format!("expected object, found {}", describe_value(value)))?;

        let hash = expect_string(map, "hash")?;
        let height = expect_u64(map, "height")?;
        let timestamp = expect_u64(map, "timestamp")?;

        Ok(Self {
            hash,
            height,
            timestamp,
        })
    }
}

impl ReceiptRecord {
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("key".to_string(), Value::String(self.key.clone()));
        map.insert("epoch".to_string(), Value::Number(Number::from(self.epoch)));
        map.insert("provider".to_string(), Value::String(self.provider.clone()));
        map.insert("buyer".to_string(), Value::String(self.buyer.clone()));
        map.insert(
            "amount".to_string(),
            Value::Number(Number::from(self.amount)),
        );
        Value::Object(map)
    }
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
                        if let Ok(list) = binary::decode::<Vec<Receipt>>(&bytes) {
                            for r in list {
                                let rec = ReceiptRecord {
                                    key: hex_encode(&r.idempotency_key),
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
    let mut values = Vec::with_capacity(blocks.len());
    for record in &blocks {
        values.push(record.to_value());
    }

    let body = json::to_vec_value(&Value::Array(values));
    Ok(Response::new(StatusCode::OK)
        .with_body(body)
        .with_header("content-type", "application/json"))
}

fn expect_string(map: &Map, key: &str) -> Result<String, String> {
    match map.get(key) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(other) => Err(format!(
            "expected '{key}' to be a string, found {}",
            describe_value(other)
        )),
        None => Err(format!("missing '{key}' field")),
    }
}

fn expect_u64(map: &Map, key: &str) -> Result<u64, String> {
    match map.get(key) {
        Some(Value::Number(number)) => number.as_u64().ok_or_else(|| {
            format!(
                "expected '{key}' to be a non-negative integer, found {}",
                number.as_f64()
            )
        }),
        Some(other) => Err(format!(
            "expected '{key}' to be a number, found {}",
            describe_value(other)
        )),
        None => Err(format!("missing '{key}' field")),
    }
}

fn describe_value(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpd::StatusCode;

    #[test]
    fn index_and_query() {
        let dir = sys::tempfile::tempdir().unwrap();
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
        let dir = sys::tempfile::tempdir().unwrap();
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
            let dir = sys::tempfile::tempdir().unwrap();
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
            let value = json::value_from_slice(response.body()).unwrap();
            let entries = value.as_array().unwrap();
            assert_eq!(entries.len(), 1);
            let block = BlockRecord::from_value(&entries[0]).unwrap();
            assert_eq!(block.hash, "abc");
        });
    }
}
