use crate::receipts::{BlockTorchReceiptMetadata, ComputeReceipt, ComputeSlashReceipt};
use std::collections::VecDeque;

const SLA_TIMEOUT_BLOCKS: u64 = 10;
const DEFAULT_VERIFICATION_LIMIT: u64 = 10_000;
const DEFAULT_VERIFICATION_WINDOW: u64 = 5;

/// Dispute opened by a client expecting a particular manifest, workload hash,
/// and resource consumption level.
#[derive(Clone, Debug)]
pub struct PendingDispute {
    pub job_id: String,
    pub provider: String,
    pub buyer: String,
    pub expected_workload_hash: [u8; 32],
    pub expected_manifest_hash: [u8; 32],
    pub expected_resource_units: u64,
    pub opened_at: u64,
}

#[derive(Clone, Debug)]
pub struct DisputePolicy {
    pub timeout_blocks: u64,
    pub verification_limit: u64,
    pub verification_window: u64,
}

impl DisputePolicy {
    pub fn new(timeout_blocks: u64, verification_limit: u64, verification_window: u64) -> Self {
        Self {
            timeout_blocks,
            verification_limit,
            verification_window,
        }
    }

    pub fn with_timeout(timeout_blocks: u64) -> Self {
        let mut policy = Self::default();
        policy.timeout_blocks = timeout_blocks;
        policy
    }
}

impl Default for DisputePolicy {
    fn default() -> Self {
        Self::new(
            SLA_TIMEOUT_BLOCKS,
            DEFAULT_VERIFICATION_LIMIT,
            DEFAULT_VERIFICATION_WINDOW,
        )
    }
}

#[derive(Debug)]
struct VerificationBudget {
    limit: u64,
    window_blocks: u64,
    history: VecDeque<(u64, u64)>,
    used: u64,
}

impl VerificationBudget {
    fn new(limit: u64, window_blocks: u64) -> Self {
        Self {
            limit,
            window_blocks,
            history: VecDeque::new(),
            used: 0,
        }
    }

    fn reserve(&mut self, block: u64, units: u64) -> bool {
        self.trim(block);
        if self.used + units > self.limit {
            return false;
        }
        if units > 0 {
            self.history.push_back((block, units));
            self.used += units;
        }
        true
    }

    fn trim(&mut self, current_block: u64) {
        while let Some(&(entry_block, entry_units)) = self.history.front() {
            if entry_block + self.window_blocks <= current_block {
                self.used = self.used.saturating_sub(entry_units);
                self.history.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Controller that tracks open compute disputes and emits deterministic slashes.
#[derive(Debug)]
pub struct DisputeController {
    pending: Vec<PendingDispute>,
    policy: DisputePolicy,
    budget: VerificationBudget,
}

impl DisputeController {
    pub fn new(timeout_blocks: u64) -> Self {
        let mut policy = DisputePolicy::default();
        policy.timeout_blocks = timeout_blocks;
        Self::with_policy(policy)
    }

    pub fn default() -> Self {
        Self::with_policy(DisputePolicy::default())
    }

    pub fn with_policy(policy: DisputePolicy) -> Self {
        Self {
            pending: Vec::new(),
            budget: VerificationBudget::new(policy.verification_limit, policy.verification_window),
            policy,
        }
    }

    pub fn policy(&self) -> &DisputePolicy {
        &self.policy
    }

    pub fn open(&mut self, dispute: PendingDispute) {
        self.pending.push(dispute);
    }

    pub fn drain_pending(
        &mut self,
        receipts: &[ComputeReceipt],
        current_block: u64,
    ) -> Vec<ComputeSlashReceipt> {
        let mut slashes = Vec::new();
        let mut remaining = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        for dispute in pending {
            match receipts
                .iter()
                .find(|receipt| receipt.job_id == dispute.job_id)
            {
                Some(receipt) => {
                    if let Some(reason) = self.evaluate_receipt(&dispute, receipt, current_block) {
                        slashes.push(self.build_slash(&dispute, current_block, reason));
                    }
                }
                None => {
                    let deadline = dispute.opened_at.saturating_add(self.policy.timeout_blocks);
                    if current_block >= deadline {
                        slashes.push(self.build_slash(&dispute, current_block, "missing_receipt"));
                    } else {
                        remaining.push(dispute);
                    }
                }
            }
        }
        self.pending = remaining;
        slashes
    }

    fn evaluate_receipt(
        &mut self,
        dispute: &PendingDispute,
        receipt: &ComputeReceipt,
        current_block: u64,
    ) -> Option<&'static str> {
        if !self.budget.reserve(current_block, receipt.compute_units) {
            return Some("verification_budget_exceeded");
        }
        if !receipt.verified {
            return Some("invalid_proof");
        }
        let meta = match receipt.blocktorch.as_ref() {
            Some(meta) => meta,
            None => return Some("missing_blocktorch"),
        };
        if meta.kernel_variant_digest != dispute.expected_workload_hash {
            return Some("workload_mismatch");
        }
        if meta.descriptor_digest != dispute.expected_manifest_hash {
            return Some("manifest_mismatch");
        }
        if receipt.compute_units < dispute.expected_resource_units {
            return Some("underreported_units");
        }
        None
    }

    fn build_slash(
        &self,
        dispute: &PendingDispute,
        current_block: u64,
        reason: &'static str,
    ) -> ComputeSlashReceipt {
        let deadline = dispute.opened_at.saturating_add(self.policy.timeout_blocks);
        ComputeSlashReceipt {
            job_id: dispute.job_id.clone(),
            provider: dispute.provider.clone(),
            buyer: dispute.buyer.clone(),
            burned: dispute.expected_resource_units,
            reason: reason.to_string(),
            deadline,
            resolved_at: current_block,
            block_height: current_block,
        }
    }
}
