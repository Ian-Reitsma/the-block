use std::collections::BTreeMap;

/// Stake-weighted leader rotation schedule.
#[derive(Clone, Debug)]
pub struct LeaderSchedule {
    ring: Vec<String>,
}

impl LeaderSchedule {
    /// Constructs a schedule from a mapping of peer identifiers to stake
    /// weight. Each peer is allotted slots proportional to its stake.
    pub fn new(stakes: BTreeMap<String, u64>) -> Self {
        let total: u64 = stakes.values().sum();
        let mut ring = Vec::new();
        for (id, stake) in stakes {
            let slots = ((stake * 1024) / total.max(1)).max(1) as usize;
            for _ in 0..slots {
                ring.push(id.clone());
            }
        }
        Self { ring }
    }

    /// Returns the leader for the given round.
    pub fn leader(&self, round: u64) -> Option<&str> {
        if self.ring.is_empty() {
            return None;
        }
        let idx = (round as usize) % self.ring.len();
        self.ring.get(idx).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_by_stake() {
        let mut stakes = BTreeMap::new();
        stakes.insert("A".into(), 1);
        stakes.insert("B".into(), 3);
        let sched = LeaderSchedule::new(stakes);
        let mut counts = BTreeMap::new();
        for r in 0..1000 {
            let l = sched.leader(r).unwrap().to_string();
            *counts.entry(l).or_insert(0usize) += 1;
        }
        // B should appear roughly 3x more often than A.
        assert!(counts.get("B").unwrap() > &(counts.get("A").unwrap() * 2));
    }
}
