use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use the_block::light_client::proof_tracker::{ProofTracker, ReceiptPage};

#[derive(Debug, Clone, Serialize)]
pub struct RelayerLeaderboardEntry {
    pub id: String,
    pub pending: u64,
    pub total_proofs: u64,
    pub total_claimed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_claim_height: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebateReceiptRelayerRow {
    pub id: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebateReceiptRow {
    pub height: u64,
    pub amount: u64,
    pub relayers: Vec<RebateReceiptRelayerRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebateHistoryPage {
    pub receipts: Vec<RebateReceiptRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

fn open_tracker(path: impl AsRef<Path>) -> ProofTracker {
    ProofTracker::open(path)
}

pub fn top_relayers(path: impl AsRef<Path>, limit: usize) -> Result<Vec<RelayerLeaderboardEntry>> {
    let tracker = open_tracker(path);
    let mut entries: Vec<_> = tracker
        .snapshot()
        .relayers
        .into_iter()
        .map(|(id, info)| RelayerLeaderboardEntry {
            id: hex::encode(id),
            pending: info.pending,
            total_proofs: info.total_proofs,
            total_claimed: info.total_claimed,
            last_claim_height: info.last_claim_height,
        })
        .collect();
    entries.sort_by(|a, b| {
        b.total_claimed
            .cmp(&a.total_claimed)
            .then(b.pending.cmp(&a.pending))
            .then(a.id.cmp(&b.id))
    });
    if entries.len() > limit {
        entries.truncate(limit);
    }
    Ok(entries)
}

pub fn recent_rebate_history(
    path: impl AsRef<Path>,
    relayer: Option<&str>,
    cursor: Option<u64>,
    limit: usize,
) -> Result<RebateHistoryPage> {
    let tracker = open_tracker(path);
    let relayer_bytes = if let Some(id) = relayer {
        Some(hex::decode(id).with_context(|| format!("invalid relayer hex string: {id}"))?)
    } else {
        None
    };
    let page: ReceiptPage = tracker.receipt_history(relayer_bytes.as_deref(), cursor, limit);
    let receipts = page
        .receipts
        .into_iter()
        .map(|entry| RebateReceiptRow {
            height: entry.height,
            amount: entry.amount,
            relayers: entry
                .relayers
                .into_iter()
                .map(|rel| RebateReceiptRelayerRow {
                    id: hex::encode(rel.id),
                    amount: rel.amount,
                })
                .collect(),
        })
        .collect();
    Ok(RebateHistoryPage {
        receipts,
        next: page.next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sys::temp;

    #[test]
    fn leaderboard_and_history_from_tracker() {
        let dir = temp::tempdir().expect("tempdir");
        let path = dir.path().join("rebates");
        {
            let mut tracker = ProofTracker::open(&path);
            tracker.record(b"alpha", 2, 10);
            tracker.record(b"beta", 1, 5);
            tracker.claim_all(10);
        }
        let leaders = top_relayers(&path, 5).expect("leaders");
        assert_eq!(leaders.len(), 2);
        assert_eq!(leaders[0].id, hex::encode(b"alpha"));
        assert_eq!(leaders[0].total_claimed, 10);

        let history = recent_rebate_history(&path, None, None, 5).expect("history");
        assert_eq!(history.receipts.len(), 1);
        assert_eq!(history.receipts[0].amount, 15);
        assert_eq!(history.receipts[0].relayers.len(), 2);
    }
}
