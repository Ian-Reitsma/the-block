use crate::Blockchain;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct HeaderSummary {
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
}

#[derive(Serialize, Clone)]
pub struct RebateRelayer {
    pub id: String,
    pub pending: u64,
    pub total_proofs: u64,
    pub total_claimed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_claim_height: Option<u64>,
}

#[derive(Serialize, Clone)]
pub struct RebateStatus {
    pub pending_total: u64,
    pub relayers: Vec<RebateRelayer>,
}

#[derive(Serialize, Clone)]
pub struct RebateReceiptRelayer {
    pub id: String,
    pub amount: u64,
}

#[derive(Serialize, Clone)]
pub struct RebateReceiptEntry {
    pub height: u64,
    pub amount: u64,
    pub relayers: Vec<RebateReceiptRelayer>,
}

#[derive(Serialize, Clone)]
pub struct RebateReceiptPage {
    pub receipts: Vec<RebateReceiptEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

pub fn latest_header(bc: &Blockchain) -> HeaderSummary {
    let blk = bc.chain.last().expect("chain");
    HeaderSummary {
        height: blk.index,
        hash: blk.hash.clone(),
        difficulty: blk.difficulty,
    }
}

pub fn rebate_status(bc: &Blockchain) -> RebateStatus {
    let snapshot = bc.proof_tracker.snapshot();
    let relayers = snapshot
        .relayers
        .into_iter()
        .map(|(id, info)| RebateRelayer {
            id: hex::encode(id),
            pending: info.pending,
            total_proofs: info.total_proofs,
            total_claimed: info.total_claimed,
            last_claim_height: info.last_claim_height,
        })
        .collect();
    RebateStatus {
        pending_total: snapshot.pending_total,
        relayers,
    }
}

pub fn rebate_history(
    bc: &Blockchain,
    relayer: Option<&[u8]>,
    cursor: Option<u64>,
    limit: usize,
) -> RebateReceiptPage {
    let page = bc.proof_tracker.receipt_history(relayer, cursor, limit);
    let receipts = page
        .receipts
        .into_iter()
        .map(|entry| RebateReceiptEntry {
            height: entry.height,
            amount: entry.amount,
            relayers: entry
                .relayers
                .into_iter()
                .map(|relayer| RebateReceiptRelayer {
                    id: hex::encode(relayer.id),
                    amount: relayer.amount,
                })
                .collect(),
        })
        .collect();
    RebateReceiptPage {
        receipts,
        next: page.next,
    }
}

pub fn headers_since(bc: &Blockchain, start: u64, limit: usize) -> Vec<HeaderSummary> {
    bc.chain
        .iter()
        .filter(|b| b.index >= start)
        .take(limit)
        .map(|b| HeaderSummary {
            height: b.index,
            hash: b.hash.clone(),
            difficulty: b.difficulty,
        })
        .collect()
}
