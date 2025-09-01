use serde::{Deserialize, Serialize};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// Credit issuance payload attached to a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditIssue {
    pub provider: String,
    pub amount: u64,
}

/// Basic proposal shared by both houses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub start: u64,
    pub end: u64,
    pub ops_for: u32,
    pub builders_for: u32,
    pub executed: bool,
    pub credit_issue: Option<CreditIssue>,
}

impl Proposal {
    pub fn new(id: u64, start: u64, end: u64, credit_issue: Option<CreditIssue>) -> Self {
        Self {
            id,
            start,
            end,
            ops_for: 0,
            builders_for: 0,
            executed: false,
            credit_issue,
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

#[derive(Clone, Copy)]
pub enum House {
    Operators,
    Builders,
}

pub struct Governance {
    pub bicameral: Bicameral,
    proposals: std::collections::HashMap<u64, Proposal>,
    next_id: u64,
}

impl Governance {
    pub fn new(quorum_ops: u32, quorum_builders: u32, timelock_secs: u64) -> Self {
        Self {
            bicameral: Bicameral::new(quorum_ops, quorum_builders, timelock_secs),
            proposals: std::collections::HashMap::new(),
            next_id: 0,
        }
    }

    pub fn load(path: &str, quorum_ops: u32, quorum_builders: u32, timelock_secs: u64) -> Self {
        if let Ok(bytes) = fs::read(path) {
            if let Ok((next_id, props)) = serde_json::from_slice::<(u64, Vec<Proposal>)>(&bytes) {
                let mut map = std::collections::HashMap::new();
                for p in props {
                    map.insert(p.id, p);
                }
                return Self {
                    bicameral: Bicameral::new(quorum_ops, quorum_builders, timelock_secs),
                    proposals: map,
                    next_id,
                };
            }
        }
        Self::new(quorum_ops, quorum_builders, timelock_secs)
    }

    pub fn persist(&self, path: &str) -> std::io::Result<()> {
        let props: Vec<&Proposal> = self.proposals.values().collect();
        let bytes = serde_json::to_vec(&(self.next_id, props))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, bytes)
    }

    pub fn submit(&mut self, start: u64, end: u64, credit_issue: Option<CreditIssue>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.proposals
            .insert(id, Proposal::new(id, start, end, credit_issue));
        id
    }

    pub fn vote(&mut self, id: u64, house: House, approve: bool) -> Result<(), &'static str> {
        let p = self.proposals.get_mut(&id).ok_or("proposal not found")?;
        match house {
            House::Operators => p.vote_operator(approve),
            House::Builders => p.vote_builder(approve),
        }
        Ok(())
    }

    /// Execute a proposal if it satisfies quorum and timelock requirements.
    ///
    /// If the proposal carries a [`CreditIssue`] payload and a ledger is
    /// supplied, credits are issued atomically upon execution. Telemetry
    /// counters track both successful issuances and rejected attempts when
    /// quorum or timelock checks fail.
    pub fn execute(
        &mut self,
        id: u64,
        now: u64,
        mut ledger: Option<&mut credits::Ledger>,
    ) -> Result<(), &'static str> {
        let p = self.proposals.get_mut(&id).ok_or("proposal not found")?;
        if self.bicameral.can_execute(p, now) {
            if let (Some(issue), Some(led)) = (p.credit_issue.clone(), ledger.as_mut()) {
                let event = format!("gov:{id}");
                led.accrue(&issue.provider, &event, issue.amount);
                #[cfg(feature = "telemetry")]
                crate::telemetry::CREDIT_ISSUED_TOTAL
                    .with_label_values(&["governance", "global"])
                    .inc_by(issue.amount);
            }
            p.executed = true;
            Ok(())
        } else {
            #[cfg(feature = "telemetry")]
            if p.credit_issue.is_some() {
                crate::telemetry::CREDIT_ISSUE_REJECTED_TOTAL
                    .with_label_values(&["quorum"])
                    .inc();
            }
            Err("quorum or timelock not satisfied")
        }
    }

    pub fn status(&self, id: u64, now: u64) -> Result<(Proposal, u64), &'static str> {
        let p = self.proposals.get(&id).ok_or("proposal not found")?.clone();
        let end = p.end + self.bicameral.timelock_secs;
        let remaining = if now >= end { 0 } else { end - now };
        Ok((p, remaining))
    }

    pub fn proposal_credit_issue(&self, id: u64) -> Option<CreditIssue> {
        self.proposals.get(&id).and_then(|p| p.credit_issue.clone())
    }

    /// Return a snapshot of all proposals for UI display.
    pub fn list(&self) -> Vec<Proposal> {
        self.proposals.values().cloned().collect()
    }
}
