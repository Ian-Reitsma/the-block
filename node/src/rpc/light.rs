use crate::Blockchain;
use serde::Serialize;

#[derive(Serialize)]
pub struct LatestHeader {
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
}

pub fn latest_header(bc: &Blockchain) -> LatestHeader {
    let blk = bc.chain.last().expect("chain");
    LatestHeader {
        height: blk.index,
        hash: blk.hash.clone(),
        difficulty: blk.difficulty,
    }
}
