#![forbid(unsafe_code)]

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct TrustLedger {
    lines: HashMap<(String, String), TrustLine>,
}

#[derive(Debug, Clone)]
pub struct TrustLine {
    pub balance: i64,
    pub limit: u64,
}

impl TrustLedger {
    pub fn establish(&mut self, a: String, b: String, limit: u64) {
        self.lines.insert((a, b), TrustLine { balance: 0, limit });
    }
    pub fn adjust(&mut self, a: &str, b: &str, amount: i64) -> bool {
        if let Some(line) = self.lines.get_mut(&(a.to_string(), b.to_string())) {
            let new = line.balance + amount;
            if new.abs() as u64 > line.limit {
                return false;
            }
            line.balance = new;
            true
        } else {
            false
        }
    }
    pub fn balance(&self, a: &str, b: &str) -> i64 {
        self.lines
            .get(&(a.to_string(), b.to_string()))
            .map(|l| l.balance)
            .unwrap_or(0)
    }
}
