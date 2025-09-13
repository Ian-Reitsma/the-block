use crate::Blockchain;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct HeaderSummary {
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
}

pub fn latest_header(bc: &Blockchain) -> HeaderSummary {
    let blk = bc.chain.last().expect("chain");
    HeaderSummary {
        height: blk.index,
        hash: blk.hash.clone(),
        difficulty: blk.difficulty,
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
