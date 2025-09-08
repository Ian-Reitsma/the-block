#![forbid(unsafe_code)]

use super::order_book::{Order, OrderBook};
use crate::simple_db::SimpleDb;
use dex::escrow::{Escrow, PaymentProof};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TradeLog(Order, Order, u64, PaymentProof);

#[derive(Default)]
pub struct DexStore {
    db: SimpleDb,
}

impl DexStore {
    pub fn open(path: &str) -> Self {
        Self {
            db: SimpleDb::open(path),
        }
    }

    pub fn save_book(&mut self, book: &OrderBook) {
        if let Ok(bytes) = bincode::serialize(book) {
            let _ = self.db.insert("book", bytes);
            self.db.flush();
        }
    }

    pub fn load_book(&self) -> OrderBook {
        self.db
            .get("book")
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_default()
    }

    pub fn log_trade(&mut self, trade: &(Order, Order, u64), proof: &PaymentProof) {
        if let Ok(bytes) = bincode::serialize(&TradeLog(
            trade.0.clone(),
            trade.1.clone(),
            trade.2,
            proof.clone(),
        )) {
            let key = format!("trade:{}", self.db.keys_with_prefix("trade:").len());
            let _ = self.db.insert(&key, bytes);
            self.db.flush();
        }
    }

    pub fn trades(&self) -> Vec<(Order, Order, u64)> {
        let mut res = Vec::new();
        for k in self.db.keys_with_prefix("trade:") {
            if let Some(bytes) = self.db.get(&k) {
                if let Ok(TradeLog(a, b, c, _)) = bincode::deserialize(&bytes) {
                    res.push((a, b, c));
                }
            }
        }
        res
    }

    pub fn save_escrow(&mut self, esc: &Escrow) {
        if let Ok(bytes) = bincode::serialize(esc) {
            let _ = self.db.insert("escrow", bytes);
            self.db.flush();
        }
    }

    pub fn load_escrow(&self) -> Escrow {
        self.db
            .get("escrow")
            .and_then(|b| bincode::deserialize(&b).ok())
            .unwrap_or_default()
    }
}
