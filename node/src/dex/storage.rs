#![forbid(unsafe_code)]

use super::order_book::{Order, OrderBook};
use super::storage_binary::{
    decode_escrow_state, decode_order_book, decode_pool, decode_trade_log, encode_escrow_state,
    encode_order_book, encode_pool, encode_trade_log, TradeLogRecord,
};
use crate::simple_db::{names, SimpleDb};
use dex::amm::Pool;
use dex::escrow::{Escrow, EscrowId, PaymentProof};
use std::collections::BTreeMap;

#[derive(Default, Debug, Clone)]
pub struct EscrowState {
    pub escrow: Escrow,
    pub locks: BTreeMap<EscrowId, (Order, Order, u64, u64)>, // buy,sell,qty,locked_at
}

#[derive(Default)]
pub struct DexStore {
    db: SimpleDb,
}

impl DexStore {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open_named(names::DEX_STORAGE, path),
        }
    }

    pub fn save_book(&mut self, book: &OrderBook) {
        if let Ok(bytes) = encode_order_book(book) {
            let _ = self.db.insert("book", bytes);
            self.db.flush();
        }
    }

    pub fn load_book(&self) -> OrderBook {
        self.db
            .get("book")
            .and_then(|b| decode_order_book(&b).ok())
            .unwrap_or_default()
    }

    pub fn log_trade(&mut self, trade: &(Order, Order, u64), proof: &PaymentProof) {
        let record = TradeLogRecord {
            buy: trade.0.clone(),
            sell: trade.1.clone(),
            quantity: trade.2,
            proof: proof.clone(),
        };
        if let Ok(bytes) = encode_trade_log(&record) {
            let key = format!("trade:{}", self.db.keys_with_prefix("trade:").len());
            let _ = self.db.insert(&key, bytes);
            self.db.flush();
        }
    }

    pub fn trades(&self) -> Vec<(Order, Order, u64)> {
        let mut res = Vec::new();
        for k in self.db.keys_with_prefix("trade:") {
            if let Some(bytes) = self.db.get(&k) {
                if let Ok(record) = decode_trade_log(&bytes) {
                    res.push((record.buy, record.sell, record.quantity));
                }
            }
        }
        res
    }

    pub fn save_escrow_state(&mut self, state: &EscrowState) {
        if let Ok(bytes) = encode_escrow_state(state) {
            let _ = self.db.insert("escrow", bytes);
            self.db.flush();
        }
    }

    pub fn load_escrow_state(&self) -> EscrowState {
        self.db
            .get("escrow")
            .and_then(|b| decode_escrow_state(&b).ok())
            .unwrap_or_default()
    }

    /// Persist an AMM pool under `amm/<id>`.
    pub fn save_pool(&mut self, id: &str, pool: &Pool) {
        if let Ok(bytes) = encode_pool(pool) {
            let key = format!("amm/{}", id);
            let _ = self.db.insert(&key, bytes);
            self.db.flush();
        }
    }

    /// Load an AMM pool, returning default if missing.
    pub fn load_pool(&self, id: &str) -> Pool {
        let key = format!("amm/{}", id);
        self.db
            .get(&key)
            .and_then(|b| decode_pool(&b).ok())
            .unwrap_or_default()
    }
}
