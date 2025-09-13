use blake3::Hasher;
use hex::encode as hex_encode;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use the_block::{
    compute_market::{receipt::Receipt, Job},
    dex::order_book::{OrderBook, Side},
    transaction::SignedTransaction,
    Block,
};

pub mod htlc_view;
pub mod storage_view;
pub mod snark_view;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRecord {
    pub key: String,
    pub epoch: u64,
    pub provider: String,
    pub buyer: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRecord {
    pub hash: String,
    pub height: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxRecord {
    pub hash: String,
    pub block_hash: String,
    pub memo: String,
    pub contract: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovProposal {
    pub id: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    pub peer_id: String,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRecord {
    pub side: String,
    pub price: u64,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeJobRecord {
    pub job_id: String,
    pub buyer: String,
    pub provider: String,
    pub status: String,
}

pub struct Explorer {
    path: PathBuf,
}

impl Explorer {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let conn = Connection::open(&p)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS receipts (key TEXT PRIMARY KEY, epoch INTEGER, provider TEXT, buyer TEXT, amount INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (hash TEXT PRIMARY KEY, height INTEGER, data BLOB)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS txs (hash TEXT PRIMARY KEY, block_hash TEXT, memo TEXT, contract TEXT, data BLOB)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS gov (id INTEGER PRIMARY KEY, data BLOB)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS peer_reputation (peer_id TEXT PRIMARY KEY, score INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dex_orders (id INTEGER PRIMARY KEY AUTOINCREMENT, side TEXT, price INTEGER, amount INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS compute_jobs (job_id TEXT PRIMARY KEY, buyer TEXT, provider TEXT, status TEXT)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS snark_proofs (job_id TEXT PRIMARY KEY, verified INTEGER)",
            [],
        )?;
        Ok(Self { path: p })
    }

    fn conn(&self) -> Result<Connection> {
        Connection::open(&self.path)
    }

    fn tx_hash(tx: &SignedTransaction) -> String {
        let mut hasher = Hasher::new();
        let bytes = bincode::serialize(tx).unwrap_or_default();
        hasher.update(&bytes);
        hasher.finalize().to_hex().to_string()
    }

    pub fn index_block(&self, block: &Block) -> Result<()> {
        let conn = self.conn()?;
        let data = bincode::serialize(block).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
            params![block.hash, block.index, data],
        )?;
        for tx in &block.transactions {
            let hash = Self::tx_hash(tx);
            let memo = String::from_utf8(tx.payload.memo.clone()).unwrap_or_default();
            let contract = tx.payload.to.clone();
            let data = bincode::serialize(tx).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO txs (hash, block_hash, memo, contract, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![hash, block.hash, memo, contract, data],
            )?;
        }
        Ok(())
    }

    pub fn ingest_block_dir(&self, dir: &Path) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for ent in entries.flatten() {
                if let Ok(bytes) = std::fs::read(ent.path()) {
                    if let Ok(block) = bincode::deserialize::<Block>(&bytes) {
                        let _ = self.index_block(&block);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_block(&self, hash: &str) -> Result<Option<Block>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE hash=?1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes.map(|b| bincode::deserialize(&b).unwrap()))
    }

    /// Fetch the base fee at the specified block height if present.
    pub fn base_fee_by_height(&self, height: u64) -> Result<Option<u64>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT data FROM blocks WHERE height=?1",
                params![height],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes
            .and_then(|b| bincode::deserialize::<Block>(&b).ok())
            .map(|b| b.base_fee))
    }

    pub fn get_tx(&self, hash: &str) -> Result<Option<SignedTransaction>> {
        let conn = self.conn()?;
        let bytes: Option<Vec<u8>> = conn
            .query_row("SELECT data FROM txs WHERE hash=?1", params![hash], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(bytes.map(|b| bincode::deserialize(&b).unwrap()))
    }

    pub fn search_memo(&self, memo: &str) -> Result<Vec<SignedTransaction>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT data FROM txs WHERE memo LIKE ?1")?;
        let rows = stmt.query_map(params![memo], |row| row.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(bytes) = r {
                if let Ok(tx) = bincode::deserialize::<SignedTransaction>(&bytes) {
                    out.push(tx);
                }
            }
        }
        Ok(out)
    }

    pub fn search_contract(&self, contract: &str) -> Result<Vec<SignedTransaction>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT data FROM txs WHERE contract=?1")?;
        let rows = stmt.query_map(params![contract], |row| row.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(bytes) = r {
                if let Ok(tx) = bincode::deserialize::<SignedTransaction>(&bytes) {
                    out.push(tx);
                }
            }
        }
        Ok(out)
    }

    pub fn index_gov_proposal(&self, prop: &GovProposal) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO gov (id, data) VALUES (?1, ?2)",
            params![prop.id, prop.data],
        )?;
        Ok(())
    }

    pub fn get_gov_proposal(&self, id: u64) -> Result<Option<GovProposal>> {
        let conn = self.conn()?;
        let row: Option<(u64, Vec<u8>)> = conn
            .query_row("SELECT id, data FROM gov WHERE id=?1", params![id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        Ok(row.map(|(id, data)| GovProposal { id, data }))
    }

    pub fn set_peer_reputation(&self, peer_id: &str, score: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO peer_reputation (peer_id, score) VALUES (?1, ?2)",
            params![peer_id, score],
        )?;
        Ok(())
    }

    pub fn peer_reputations(&self) -> Result<Vec<PeerReputation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT peer_id, score FROM peer_reputation")?;
        let rows = stmt.query_map([], |row| {
            Ok(PeerReputation {
                peer_id: row.get(0)?,
                score: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_order_book(&self, book: &OrderBook) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM dex_orders", [])?;
        for (price, orders) in &book.bids {
            for ord in orders {
                conn.execute(
                    "INSERT INTO dex_orders (side, price, amount) VALUES ('buy', ?1, ?2)",
                    params![price, ord.amount],
                )?;
            }
        }
        for (price, orders) in &book.asks {
            for ord in orders {
                conn.execute(
                    "INSERT INTO dex_orders (side, price, amount) VALUES ('sell', ?1, ?2)",
                    params![price, ord.amount],
                )?;
            }
        }
        Ok(())
    }

    pub fn order_book(&self) -> Result<Vec<OrderRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT side, price, amount FROM dex_orders ORDER BY price")?;
        let rows = stmt.query_map([], |row| {
            Ok(OrderRecord {
                side: row.get(0)?,
                price: row.get(1)?,
                amount: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_job(&self, job: &Job, provider: &str, status: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO compute_jobs (job_id, buyer, provider, status) VALUES (?1, ?2, ?3, ?4)",
            params![job.job_id, job.buyer, provider, status],
        )?;
        Ok(())
    }

    pub fn compute_jobs(&self) -> Result<Vec<ComputeJobRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT job_id, buyer, provider, status FROM compute_jobs")?;
        let rows = stmt.query_map([], |row| {
            Ok(ComputeJobRecord {
                job_id: row.get(0)?,
                buyer: row.get(1)?,
                provider: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn ingest_dir(&self, dir: &Path) -> Result<()> {
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

    pub fn index_receipt(&self, rec: &ReceiptRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO receipts (key, epoch, provider, buyer, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![rec.key, rec.epoch, rec.provider, rec.buyer, rec.amount],
        )?;
        Ok(())
    }

    pub fn receipts_by_provider(&self, prov: &str) -> Result<Vec<ReceiptRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, epoch, provider, buyer, amount FROM receipts WHERE provider=?1 ORDER BY epoch",
        )?;
        let rows = stmt.query_map(params![prov], |row| {
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

    pub fn receipts_by_domain(&self, dom: &str) -> Result<Vec<ReceiptRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, epoch, provider, buyer, amount FROM receipts WHERE buyer=?1 ORDER BY epoch",
        )?;
        let rows = stmt.query_map(params![dom], |row| {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn index_and_query() {
        let dir = tempdir().unwrap();
        let receipts = dir.path().join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();
        let r = Receipt::new("job".into(), "buyer".into(), "prov".into(), 10, 1, false);
        let bytes = bincode::serialize(&vec![r]).unwrap();
        std::fs::write(receipts.join("1"), bytes).unwrap();
        let db = dir.path().join("explorer.db");
        let ex = Explorer::open(&db).unwrap();
        ex.ingest_dir(&receipts).unwrap();
        assert_eq!(ex.receipts_by_provider("prov").unwrap().len(), 1);
        assert_eq!(ex.receipts_by_domain("buyer").unwrap().len(), 1);
    }
}
