#![cfg(feature = "integration-tests")]
use the_block::net::Node;

/// Mine `len` blocks on `node` starting at timestamp `ts` without gossiping.
pub fn inject_fork(node: &Node, miner: &str, ts: u64, len: u64) {
    let mut bc = node.blockchain();
    let mut t = ts;
    for _ in 0..len {
        bc.mine_block_at(miner, t).expect("mine block");
        t += 1;
    }
}
