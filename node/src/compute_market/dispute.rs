use crate::receipts::{BlockTorchReceiptMetadata, ComputeReceipt, ComputeSlashReceipt};

const SLA_TIMEOUT_BLOCKS: u64 = 10;

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

/// Controller that tracks open compute disputes and emits deterministic slashes.
#[derive(Debug)]
pub struct DisputeController {
    pending: Vec<PendingDispute>,
    timeout: u64,
}

impl DisputeController {
    pub fn new(timeout_blocks: u64) -> Self {
        Self {
            pending: Vec::new(),
            timeout: timeout_blocks,
        }
    }

    pub fn default() -> Self {
        Self::new(SLA_TIMEOUT_BLOCKS)
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
                    if let Some(reason) = self.evaluate_receipt(&dispute, receipt) {
                        slashes.push(self.build_slash(&dispute, current_block, reason));
                    }
                }
                None => {
                    let deadline = dispute.opened_at.saturating_add(self.timeout);
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
        &self,
        dispute: &PendingDispute,
        receipt: &ComputeReceipt,
    ) -> Option<&'static str> {
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
        let deadline = dispute.opened_at.saturating_add(self.timeout);
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
