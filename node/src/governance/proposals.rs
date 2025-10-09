use crate::governance::ParamKey;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::Address;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum ProposalStatus {
    Open,
    Passed,
    Rejected,
    Activated,
    RolledBack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Proposal {
    pub id: u64,
    pub key: ParamKey,
    pub new_value: i64,
    pub min: i64,
    pub max: i64,
    pub proposer: Address,
    pub created_epoch: u64,
    pub vote_deadline_epoch: u64,
    pub activation_epoch: Option<u64>,
    pub status: ProposalStatus,
    /// Proposal IDs this proposal depends on.
    pub deps: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Vote {
    pub proposal_id: u64,
    pub voter: Address,
    pub choice: VoteChoice,
    pub weight: u64,
    pub received_at: u64,
}

/// Validate that inserting the `new_prop` into the existing graph does not create cycles.
pub fn validate_dag(existing: &HashMap<u64, Proposal>, new_prop: &Proposal) -> bool {
    let mut graph: HashMap<u64, Vec<u64>> = existing
        .iter()
        .map(|(id, p)| (*id, p.deps.clone()))
        .collect();
    graph.insert(new_prop.id, new_prop.deps.clone());

    fn visit(
        node: u64,
        graph: &HashMap<u64, Vec<u64>>,
        temp: &mut HashSet<u64>,
        perm: &mut HashSet<u64>,
    ) -> bool {
        if perm.contains(&node) {
            return true;
        }
        if !temp.insert(node) {
            return false; // cycle
        }
        if let Some(children) = graph.get(&node) {
            for &c in children {
                if !visit(c, graph, temp, perm) {
                    return false;
                }
            }
        }
        temp.remove(&node);
        perm.insert(node);
        true
    }

    let mut temp = HashSet::new();
    let mut perm = HashSet::new();
    for &node in graph.keys() {
        if !visit(node, &graph, &mut temp, &mut perm) {
            return false;
        }
    }
    true
}
