//! Treasury Dependency Management
//!
//! **CRITICAL**: This module integrates with existing dependency checking in
//! `node/src/treasury_executor.rs`. The existing system uses memo-based dependency
//! parsing. This module adds validation and DAG analysis on top.
//!
//! DO NOT duplicate `parse_dependency_list()` logic. Always use the canonical
//! implementation from treasury_executor.

use crate::treasury::{parse_dependency_list, TreasuryDisbursement};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Error type for dependency operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum DependencyError {
    /// Circular dependency detected in the graph
    CycleDetected {
        /// IDs forming the cycle
        cycle: Vec<u64>,
    },
    /// A required dependency is missing (not found in store)
    MissingDependency {
        /// ID of disbursement with dependency
        disbursement_id: u64,
        /// Missing dependency ID
        missing_id: u64,
    },
    /// Dependency exists but is in invalid state
    InvalidDependencyState {
        /// ID of the dependent disbursement
        disbursement_id: u64,
        /// ID of dependency in wrong state
        dependency_id: u64,
        /// Current state (e.g., "draft", "voting")
        current_state: String,
        /// Required state (e.g., "executed", "finalized")
        required_state: String,
    },
    /// Empty dependency list when cycle detection attempted
    EmptyGraph,
}

impl std::fmt::Display for DependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected { cycle } => {
                write!(f, "Circular dependency: {:?}", cycle)
            }
            Self::MissingDependency {
                disbursement_id,
                missing_id,
            } => {
                write!(
                    f,
                    "Disbursement {} requires missing dependency {}",
                    disbursement_id, missing_id
                )
            }
            Self::InvalidDependencyState {
                disbursement_id,
                dependency_id,
                current_state,
                required_state,
            } => {
                write!(
                    f,
                    "Disbursement {}: dependency {} in state '{}', need '{}'",
                    disbursement_id, dependency_id, current_state, required_state
                )
            }
            Self::EmptyGraph => {
                write!(f, "Cannot validate: no disbursements in graph")
            }
        }
    }
}

impl std::error::Error for DependencyError {}

/// Status of a disbursement for dependency checking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum DependencyStatus {
    /// Not yet visible for execution
    Immature,
    /// Dependencies not yet satisfied
    Waiting,
    /// All dependencies satisfied, ready for execution
    Ready,
    /// Executed or finalized
    Completed,
    /// Rolled back (failed or cancelled)
    Failed,
}

/// Information about a single disbursement in dependency context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct DisbursementNode {
    /// Disbursement ID
    pub id: u64,
    /// IDs this disbursement depends on
    pub dependencies: Vec<u64>,
    /// Current status for dependency checking
    pub status: DependencyStatus,
    /// Human-readable state ("draft", "voting", etc.)
    pub state: String,
}

/// DAG (Directed Acyclic Graph) validator for disbursement dependencies
pub struct DependencyGraph {
    /// All disbursement nodes by ID
    nodes: HashMap<u64, DisbursementNode>,
    /// Reverse mapping: which disbursements depend on each ID
    dependents: HashMap<u64, Vec<u64>>,
}

impl DependencyGraph {
    /// Build a new dependency graph from disbursement list
    ///
    /// **Note**: Dependencies are extracted using the CANONICAL parser from
    /// treasury_executor.rs (via parse_dependency_list). This ensures consistency
    /// with the actual execution engine.
    pub fn new(disbursements: &[TreasuryDisbursement]) -> Result<Self, DependencyError> {
        if disbursements.is_empty() {
            return Err(DependencyError::EmptyGraph);
        }

        let mut nodes = HashMap::new();
        let mut dependents: HashMap<u64, Vec<u64>> = HashMap::new();

        // Build node map
        for disb in disbursements {
            let deps = parse_dependency_list(&disb.memo);
            let status = determine_dependency_status(&disb.memo);

            nodes.insert(
                disb.id,
                DisbursementNode {
                    id: disb.id,
                    dependencies: deps.clone(),
                    status,
                    state: format!("{:?}", disb), // Simplified for now
                },
            );

            // Build reverse index
            for dep_id in &deps {
                dependents.entry(*dep_id).or_insert_with(Vec::new).push(disb.id);
            }
        }

        // Validate all dependencies exist
        for (_, node) in &nodes {
            for dep_id in &node.dependencies {
                if !nodes.contains_key(dep_id) {
                    return Err(DependencyError::MissingDependency {
                        disbursement_id: node.id,
                        missing_id: *dep_id,
                    });
                }
            }
        }

        Ok(DependencyGraph { nodes, dependents })
    }

    /// Check for cycles in the dependency graph using DFS
    pub fn has_cycle(&self) -> Result<(), DependencyError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.detect_cycle_dfs(*node_id, &mut visited, &mut rec_stack)?
            }
        }

        Ok(())
    }

    /// DFS helper for cycle detection
    fn detect_cycle_dfs(
        &self,
        node_id: u64,
        visited: &mut HashSet<u64>,
        rec_stack: &mut HashSet<u64>,
    ) -> Result<(), DependencyError> {
        visited.insert(node_id);
        rec_stack.insert(node_id);

        if let Some(node) = self.nodes.get(&node_id) {
            for &dep_id in &node.dependencies {
                if !visited.contains(&dep_id) {
                    self.detect_cycle_dfs(dep_id, visited, rec_stack)?
                } else if rec_stack.contains(&dep_id) {
                    // Found cycle - trace it
                    let cycle = self.trace_cycle(node_id, dep_id);
                    return Err(DependencyError::CycleDetected { cycle });
                }
            }
        }

        rec_stack.remove(&node_id);
        Ok(())
    }

    /// Trace a cycle back to its start
    fn trace_cycle(&self, from: u64, to: u64) -> Vec<u64> {
        let mut cycle = vec![from];
        let mut current = from;

        while current != to {
            if let Some(node) = self.nodes.get(&current) {
                if let Some(&next) = node.dependencies.first() {
                    cycle.push(next);
                    current = next;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        cycle.push(to);
        cycle
    }

    /// Get topologically sorted disbursement IDs
    /// (dependencies come before dependents)
    pub fn topological_sort(&self) -> Result<Vec<u64>, DependencyError> {
        self.has_cycle()?; // Ensure no cycles first

        let mut visited = HashSet::new();
        let mut result = Vec::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.topo_dfs(*node_id, &mut visited, &mut result);
            }
        }

        result.reverse();
        Ok(result)
    }

    /// DFS helper for topological sort
    fn topo_dfs(&self, node_id: u64, visited: &mut HashSet<u64>, result: &mut Vec<u64>) {
        visited.insert(node_id);

        if let Some(node) = self.nodes.get(&node_id) {
            for &dep_id in &node.dependencies {
                if !visited.contains(&dep_id) {
                    self.topo_dfs(dep_id, visited, result);
                }
            }
        }

        result.push(node_id);
    }

    /// Count total disbursements
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Count total dependency edges
    pub fn edge_count(&self) -> usize {
        self.nodes.values().map(|n| n.dependencies.len()).sum()
    }

    /// Get all disbursements that directly depend on the given ID
    ///
    /// This is useful for impact analysis - if a disbursement fails, you can find
    /// all downstream disbursements that will be blocked.
    pub fn get_dependents(&self, id: u64) -> Vec<u64> {
        self.dependents.get(&id).cloned().unwrap_or_default()
    }

    /// Get all disbursements that would be transitively affected if the given ID fails
    ///
    /// This performs a DFS to find all downstream dependents recursively.
    /// Returns a topologically sorted list (immediate dependents first).
    pub fn get_transitive_dependents(&self, id: u64) -> Vec<u64> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.collect_dependents_dfs(id, &mut visited, &mut result);
        result
    }

    /// DFS helper to collect all transitive dependents
    fn collect_dependents_dfs(&self, id: u64, visited: &mut HashSet<u64>, result: &mut Vec<u64>) {
        if let Some(direct_deps) = self.dependents.get(&id) {
            for &dep_id in direct_deps {
                if !visited.contains(&dep_id) {
                    visited.insert(dep_id);
                    result.push(dep_id);
                    self.collect_dependents_dfs(dep_id, visited, result);
                }
            }
        }
    }

    /// Get all "ready" disbursements - those whose dependencies are all completed
    ///
    /// This is useful for finding which disbursements can be executed in parallel.
    pub fn get_ready_disbursements(&self) -> Vec<u64> {
        self.nodes
            .iter()
            .filter_map(|(&id, node)| {
                if node.status == DependencyStatus::Ready {
                    Some(id)
                } else if node.dependencies.is_empty() {
                    // No dependencies = ready by default
                    Some(id)
                } else {
                    // Check if all dependencies are completed
                    let all_deps_complete = node.dependencies.iter().all(|dep_id| {
                        self.nodes
                            .get(dep_id)
                            .map(|n| n.status == DependencyStatus::Completed)
                            .unwrap_or(false)
                    });
                    if all_deps_complete {
                        Some(id)
                    } else {
                        None
                    }
                }
            })
            .collect()
    }

    /// Check if a disbursement has any pending dependencies
    pub fn has_pending_dependencies(&self, id: u64) -> bool {
        if let Some(node) = self.nodes.get(&id) {
            node.dependencies.iter().any(|dep_id| {
                self.nodes
                    .get(dep_id)
                    .map(|n| {
                        !matches!(
                            n.status,
                            DependencyStatus::Completed | DependencyStatus::Failed
                        )
                    })
                    .unwrap_or(true)
            })
        } else {
            false
        }
    }
}

/// Determine dependency status based on memo content
fn determine_dependency_status(memo: &str) -> DependencyStatus {
    // Simplified heuristic based on memo markers
    let trimmed = memo.to_lowercase();

    if trimmed.contains("ready") || trimmed.contains("mature") {
        DependencyStatus::Ready
    } else if trimmed.contains("waiting") || trimmed.contains("pending") {
        DependencyStatus::Waiting
    } else if trimmed.contains("completed") || trimmed.contains("executed") {
        DependencyStatus::Completed
    } else if trimmed.contains("failed") || trimmed.contains("rolled") {
        DependencyStatus::Failed
    } else {
        DependencyStatus::Immature
    }
}
