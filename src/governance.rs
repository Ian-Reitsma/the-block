use std::time::{SystemTime, UNIX_EPOCH};

/// Basic proposal shared by both houses.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: u64,
    pub start: u64,
    pub end: u64,
    pub ops_for: u32,
    pub builders_for: u32,
    pub executed: bool,
}

impl Proposal {
    pub fn new(id: u64, start: u64, end: u64) -> Self {
        Self {
            id,
            start,
            end,
            ops_for: 0,
            builders_for: 0,
            executed: false,
        }
    }
    pub fn vote_operator(&mut self, approve: bool) {
        if approve {
            self.ops_for += 1;
        }
    }
    pub fn vote_builder(&mut self, approve: bool) {
        if approve {
            self.builders_for += 1;
        }
    }
}

/// Bicameral vote scaffold with quorum and timelock.
pub struct Bicameral {
    quorum_ops: u32,
    quorum_builders: u32,
    timelock_secs: u64,
}

impl Bicameral {
    pub fn new(quorum_ops: u32, quorum_builders: u32, timelock_secs: u64) -> Self {
        Self {
            quorum_ops,
            quorum_builders,
            timelock_secs,
        }
    }

    pub fn can_execute(&self, p: &Proposal, now: u64) -> bool {
        !p.executed
            && p.ops_for >= self.quorum_ops
            && p.builders_for >= self.quorum_builders
            && now >= p.end + self.timelock_secs
    }

    /// Convenience helper using the current wall clock time.
    pub fn can_execute_now(&self, p: &Proposal) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.can_execute(p, now)
    }
}
