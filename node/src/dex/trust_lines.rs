#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Default)]
pub struct TrustLedger {
    lines: HashMap<(String, String), TrustLine>,
}

#[derive(Debug, Clone)]
pub struct TrustLine {
    pub balance: i64,
    pub limit: u64,
    pub authorized: bool,
}

impl TrustLedger {
    pub fn establish(&mut self, a: String, b: String, limit: u64) {
        self.lines.insert(
            (a, b),
            TrustLine {
                balance: 0,
                limit,
                authorized: false,
            },
        );
    }

    pub fn authorize(&mut self, a: &str, b: &str) {
        if let Some(line) = self.lines.get_mut(&(a.to_string(), b.to_string())) {
            line.authorized = true;
        }
    }

    pub fn adjust(&mut self, a: &str, b: &str, amount: i64) -> bool {
        if let Some(line) = self.lines.get_mut(&(a.to_string(), b.to_string())) {
            if !line.authorized {
                return false;
            }
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

    /// Breadth-first search for a payment path from `src` to `dst` with at least `amount`
    /// of available credit on each hop. Returns the account sequence if found.
    pub fn find_path(&self, src: &str, dst: &str, amount: u64) -> Option<Vec<String>> {
        let mut q = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        q.push_back(src.to_string());
        visited.insert(src.to_string());
        while let Some(cur) = q.pop_front() {
            if cur == dst {
                break;
            }
            for ((a, b), line) in self.lines.iter() {
                if a == &cur
                    && line.authorized
                    && line.limit >= (line.balance.abs() as u64 + amount)
                    && !visited.contains(b)
                {
                    visited.insert(b.clone());
                    prev.insert(b.clone(), cur.clone());
                    q.push_back(b.clone());
                }
            }
        }
        if !visited.contains(dst) {
            return None;
        }
        let mut path = Vec::new();
        let mut cur = dst.to_string();
        path.push(cur.clone());
        while let Some(p) = prev.get(&cur) {
            cur = p.clone();
            path.push(cur.clone());
            if &cur == src {
                break;
            }
        }
        path.reverse();
        Some(path)
    }
}
