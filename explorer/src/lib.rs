use blake3::Hasher;
use hex::encode as hex_encode;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use storage;
use the_block::{
    compute_market::{receipt::Receipt, Job},
    dex::order_book::{OrderBook, Side},
    transaction::SignedTransaction,
    Block,
};

mod ai_summary;
pub mod bridge_view;
pub mod compute_view;
pub mod dex_view;
pub mod htlc_view;
pub mod release_view;
pub mod snark_view;
pub mod storage_view;
pub use release_view::{release_history, ReleaseHistoryEntry};
pub fn amm_stats() -> Vec<(String, u128, u128)> {
    Vec::new()
}
pub fn qos_tiers() -> Vec<(String, u64)> {
    Vec::new()
}
pub use ai_summary::summarize_block;
pub mod dkg_view;
pub mod jurisdiction_view;
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
pub struct PeerHandshake {
    pub peer_id: String,
    pub success: i64,
    pub failure: i64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustLineRecord {
    pub from: String,
    pub to: String,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsidyRecord {
    pub epoch: u64,
    pub beta: u64,
    pub gamma: u64,
    pub kappa: u64,
    pub lambda: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSupplyRecord {
    pub symbol: String,
    pub height: u64,
    pub supply: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeVolumeRecord {
    pub symbol: String,
    pub amount: u64,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeChallengeRecord {
    pub commitment: String,
    pub user: String,
    pub amount: u64,
    pub challenged: bool,
    pub initiated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStorageStat {
    pub provider_id: String,
    pub capacity_bytes: u64,
    pub reputation: i64,
    pub contracts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    pub ts: i64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightProof {
    pub block_hash: String,
    pub proof: Vec<u8>,
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
            "CREATE TABLE IF NOT EXISTS peer_handshakes (peer_id TEXT PRIMARY KEY, success INTEGER, failure INTEGER)",
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
        conn.execute(
            "CREATE TABLE IF NOT EXISTS trust_lines (from_id TEXT, to_id TEXT, limit INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subsidy_history (epoch INTEGER PRIMARY KEY, beta INTEGER, gamma INTEGER, kappa INTEGER, lambda INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics_archive (name TEXT, ts INTEGER, value REAL)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS light_proofs (block_hash TEXT PRIMARY KEY, proof BLOB)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS token_supply (symbol TEXT, height INTEGER, supply INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bridge_volume (symbol TEXT, amount INTEGER, ts INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bridge_challenges (commitment TEXT PRIMARY KEY, user TEXT, amount INTEGER, challenged INTEGER, initiated_at INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS storage_contracts (object_id TEXT PRIMARY KEY, provider_id TEXT, price_per_block INTEGER)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_stats (provider_id TEXT PRIMARY KEY, capacity_bytes INTEGER, reputation INTEGER)",
            [],
        )?;
        Ok(Self { path: p })
    }

    pub fn record_bridge_challenge(&self, rec: &BridgeChallengeRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO bridge_challenges (commitment, user, amount, challenged, initiated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &rec.commitment,
                &rec.user,
                rec.amount as i64,
                if rec.challenged { 1 } else { 0 },
                rec.initiated_at,
            ],
        )?;
        Ok(())
    }

    pub fn active_bridge_challenges(&self) -> Result<Vec<BridgeChallengeRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT commitment, user, amount, challenged, initiated_at FROM bridge_challenges WHERE challenged = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BridgeChallengeRecord {
                commitment: row.get(0)?,
                user: row.get(1)?,
                amount: row.get::<_, i64>(2)? as u64,
                challenged: row.get::<_, i64>(3)? != 0,
                initiated_at: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
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

    pub fn index_trust_line(&self, from: &str, to: &str, limit: u64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO trust_lines (from_id, to_id, limit) VALUES (?1, ?2, ?3)",
            params![from, to, limit],
        )?;
        Ok(())
    }

    pub fn trust_lines(&self) -> Result<Vec<TrustLineRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT from_id, to_id, limit FROM trust_lines")?;
        let rows = stmt.query_map([], |row| {
            Ok(TrustLineRecord {
                from: row.get(0)?,
                to: row.get(1)?,
                limit: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_subsidy(&self, rec: &SubsidyRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO subsidy_history (epoch, beta, gamma, kappa, lambda) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![rec.epoch, rec.beta, rec.gamma, rec.kappa, rec.lambda],
        )?;
        Ok(())
    }

    pub fn subsidy_history(&self) -> Result<Vec<SubsidyRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT epoch, beta, gamma, kappa, lambda FROM subsidy_history ORDER BY epoch",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SubsidyRecord {
                epoch: row.get(0)?,
                beta: row.get(1)?,
                gamma: row.get(2)?,
                kappa: row.get(3)?,
                lambda: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn archive_metric(&self, point: &MetricPoint) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO metrics_archive (name, ts, value) VALUES (?1, ?2, ?3)",
            params![point.name, point.ts, point.value],
        )?;
        Ok(())
    }

    pub fn metric_points(&self, name: &str) -> Result<Vec<MetricPoint>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT name, ts, value FROM metrics_archive WHERE name=?1 ORDER BY ts")?;
        let rows = stmt.query_map(params![name], |row| {
            Ok(MetricPoint {
                name: row.get(0)?,
                ts: row.get(1)?,
                value: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_storage_contract(&self, contract: &storage::StorageContract) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO storage_contracts (object_id, provider_id, price_per_block) VALUES (?1, ?2, ?3)",
            params![contract.object_id, contract.provider_id, contract.price_per_block],
        )?;
        Ok(())
    }

    pub fn set_provider_stats(
        &self,
        provider_id: &str,
        capacity_bytes: u64,
        reputation: i64,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO provider_stats (provider_id, capacity_bytes, reputation) VALUES (?1, ?2, ?3)",
            params![provider_id, capacity_bytes, reputation],
        )?;
        Ok(())
    }

    pub fn provider_storage_stats(&self) -> Result<Vec<ProviderStorageStat>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT ps.provider_id, ps.capacity_bytes, ps.reputation, COUNT(sc.object_id) as contracts \
             FROM provider_stats ps LEFT JOIN storage_contracts sc ON sc.provider_id = ps.provider_id \
             GROUP BY ps.provider_id, ps.capacity_bytes, ps.reputation",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProviderStorageStat {
                provider_id: row.get(0)?,
                capacity_bytes: row.get(1)?,
                reputation: row.get(2)?,
                contracts: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn index_light_proof(&self, proof: &LightProof) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO light_proofs (block_hash, proof) VALUES (?1, ?2)",
            params![proof.block_hash, proof.proof],
        )?;
        Ok(())
    }

    pub fn light_proof(&self, hash: &str) -> Result<Option<LightProof>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT block_hash, proof FROM light_proofs WHERE block_hash=?1",
            params![hash],
            |row| {
                Ok(LightProof {
                    block_hash: row.get(0)?,
                    proof: row.get(1)?,
                })
            },
        )
        .optional()
    }

    pub fn record_handshake(&self, peer_id: &str, success: bool) -> Result<()> {
        let conn = self.conn()?;
        let (succ, fail) = if success { (1, 0) } else { (0, 1) };
        conn.execute(
            "INSERT INTO peer_handshakes (peer_id, success, failure) VALUES (?1, ?2, ?3) \
             ON CONFLICT(peer_id) DO UPDATE SET success = success + excluded.success, failure = failure + excluded.failure",
            params![peer_id, succ, fail],
        )?;
        Ok(())
    }

    pub fn peer_handshakes(&self) -> Result<Vec<PeerHandshake>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT peer_id, success, failure FROM peer_handshakes")?;
        let rows = stmt.query_map([], |row| {
            Ok(PeerHandshake {
                peer_id: row.get(0)?,
                success: row.get(1)?,
                failure: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn record_token_supply(&self, symbol: &str, height: u64, supply: u64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO token_supply (symbol, height, supply) VALUES (?1, ?2, ?3)",
            params![symbol, height, supply],
        )?;
        Ok(())
    }

    pub fn record_bridge_volume(&self, symbol: &str, amount: u64, ts: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO bridge_volume (symbol, amount, ts) VALUES (?1, ?2, ?3)",
            params![symbol, amount, ts],
        )?;
        Ok(())
    }

    pub fn token_supply(&self, symbol: &str) -> Result<Vec<TokenSupplyRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT symbol, height, supply FROM token_supply WHERE symbol = ?1 ORDER BY height",
        )?;
        let rows = stmt.query_map(params![symbol], |row| {
            Ok(TokenSupplyRecord {
                symbol: row.get(0)?,
                height: row.get(1)?,
                supply: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn bridge_volume(&self, symbol: &str) -> Result<Vec<BridgeVolumeRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT symbol, amount, ts FROM bridge_volume WHERE symbol = ?1 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![symbol], |row| {
            Ok(BridgeVolumeRecord {
                symbol: row.get(0)?,
                amount: row.get(1)?,
                ts: row.get(2)?,
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

    /// Load opcode trace for a transaction from `trace/<hash>.json`.
    pub fn opcode_trace(&self, tx_hash: &str) -> Option<Vec<String>> {
        let path = PathBuf::from("trace").join(format!("{tx_hash}.json"));
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// Convert WASM bytecode into a human-readable WAT string.
    pub fn wasm_disasm(&self, bytes: &[u8]) -> Option<String> {
        wasmprinter::print_bytes(bytes).ok()
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
