#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::convert::TryFrom;

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
    /// of available balance on each hop. Returns the account sequence if found.
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
                if a != &cur || visited.contains(b) {
                    continue;
                }
                if Self::edge_available(line, amount).is_none() {
                    continue;
                }
                visited.insert(b.clone());
                prev.insert(b.clone(), cur.clone());
                q.push_back(b.clone());
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

    /// Find the lowest-cost path and a fallback route if available.
    pub fn find_best_path(
        &self,
        src: &str,
        dst: &str,
        amount: u64,
    ) -> Option<(Vec<String>, Option<Vec<String>>)> {
        let shortest = self.dijkstra(src, dst, amount, &HashSet::new())?;
        let slack_path = self
            .max_slack_path(src, dst, amount)
            .unwrap_or_else(|| shortest.clone());
        let fallback = if slack_path != shortest {
            Some(shortest)
        } else {
            let mut excluded = HashSet::new();
            for window in slack_path.windows(2) {
                excluded.insert((window[0].clone(), window[1].clone()));
            }
            let disjoint = self.dijkstra(src, dst, amount, &excluded);
            disjoint.filter(|path| *path != slack_path)
        };
        Some((slack_path, fallback))
    }

    pub fn lines_iter(&self) -> impl Iterator<Item = (&(String, String), &TrustLine)> {
        self.lines.iter()
    }

    pub fn settle_path(&mut self, path: &[String], amount: u64) -> bool {
        if amount == 0 {
            return true;
        }
        if path.len() < 2 {
            return false;
        }
        let amount_i64 = match i64::try_from(amount) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let mut applied: Vec<(String, String)> = Vec::new();
        for window in path.windows(2) {
            let from = window[0].clone();
            let to = window[1].clone();
            if !self.adjust(&from, &to, amount_i64) {
                for (src, dst) in applied.into_iter().rev() {
                    let _ = self.adjust(&src, &dst, -amount_i64);
                    let _ = self.adjust(&dst, &src, amount_i64);
                }
                return false;
            }
            if !self.adjust(&to, &from, -amount_i64) {
                let _ = self.adjust(&from, &to, -amount_i64);
                for (src, dst) in applied.into_iter().rev() {
                    let _ = self.adjust(&src, &dst, -amount_i64);
                    let _ = self.adjust(&dst, &src, amount_i64);
                }
                return false;
            }
            applied.push((from, to));
        }
        true
    }

    fn dijkstra(
        &self,
        src: &str,
        dst: &str,
        amount: u64,
        excluded: &HashSet<(String, String)>,
    ) -> Option<Vec<String>> {
        #[derive(Eq, PartialEq)]
        struct State {
            cost: u64,
            node: String,
        }
        impl Ord for State {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other.cost.cmp(&self.cost)
            }
        }
        impl PartialOrd for State {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        let mut dist: HashMap<String, u64> = HashMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(src.to_string(), 0);
        heap.push(State {
            cost: 0,
            node: src.to_string(),
        });
        while let Some(State { cost, node }) = heap.pop() {
            if node == dst {
                break;
            }
            if let Some(&d) = dist.get(&node) {
                if cost > d {
                    continue;
                }
            }
            for ((a, b), line) in self.lines.iter() {
                if a != &node || excluded.contains(&(a.clone(), b.clone())) {
                    continue;
                }
                if Self::edge_available(line, amount).is_none() {
                    continue;
                }
                let next = cost + 1;
                if next < *dist.get(b).unwrap_or(&u64::MAX) {
                    dist.insert(b.clone(), next);
                    prev.insert(b.clone(), node.clone());
                    heap.push(State {
                        cost: next,
                        node: b.clone(),
                    });
                }
            }
        }
        if !dist.contains_key(dst) {
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

    fn max_slack_path(&self, src: &str, dst: &str, amount: u64) -> Option<Vec<String>> {
        #[derive(Eq, PartialEq)]
        struct SlackState {
            slack: u64,
            hops: usize,
            node: String,
        }

        impl Ord for SlackState {
            fn cmp(&self, other: &Self) -> Ordering {
                match self.slack.cmp(&other.slack) {
                    Ordering::Equal => other.hops.cmp(&self.hops),
                    order => order,
                }
            }
        }

        impl PartialOrd for SlackState {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut best: HashMap<String, (u64, usize)> = HashMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();
        heap.push(SlackState {
            slack: u64::MAX,
            hops: 0,
            node: src.to_string(),
        });
        best.insert(src.to_string(), (u64::MAX, 0));

        while let Some(SlackState { slack, hops, node }) = heap.pop() {
            if node == dst {
                break;
            }
            if let Some((best_slack, best_hops)) = best.get(&node).copied() {
                if slack < best_slack || (slack == best_slack && hops > best_hops) {
                    continue;
                }
            }
            for ((a, b), line) in self.lines.iter() {
                if a != &node {
                    continue;
                }
                if let Some(edge_slack) = Self::edge_available(line, amount) {
                    let candidate_slack = slack.min(edge_slack);
                    let candidate_hops = hops + 1;
                    let entry = best.get(b).copied().unwrap_or((0, usize::MAX));
                    if candidate_slack > entry.0
                        || (candidate_slack == entry.0 && candidate_hops < entry.1)
                    {
                        best.insert(b.clone(), (candidate_slack, candidate_hops));
                        prev.insert(b.clone(), node.clone());
                        heap.push(SlackState {
                            slack: candidate_slack,
                            hops: candidate_hops,
                            node: b.clone(),
                        });
                    }
                }
            }
        }

        if !best.contains_key(dst) {
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
        if path.first().map(|s| s.as_str()) != Some(src)
            || path.last().map(|s| s.as_str()) != Some(dst)
        {
            return None;
        }
        Some(path)
    }

    fn edge_available(line: &TrustLine, amount: u64) -> Option<u64> {
        if !line.authorized {
            return None;
        }
        let balance_abs = line.balance.checked_abs()?;
        let balance_abs = u64::try_from(balance_abs).ok()?;
        let required = balance_abs.checked_add(amount)?;
        if line.limit < required {
            return None;
        }
        Some(line.limit - required)
    }
}
