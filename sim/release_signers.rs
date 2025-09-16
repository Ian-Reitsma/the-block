//! Simulate signer churn against a fixed quorum threshold.

pub fn simulate_signer_quorum() {
    let mut signers = vec!["alice", "bob", "carol", "dave"];
    let threshold = 3usize;
    for epoch in 0..8 {
        let active: Vec<_> = signers
            .iter()
            .cycle()
            .skip(epoch % signers.len())
            .take(threshold)
            .cloned()
            .collect();
        let satisfied = active.len() >= threshold;
        println!(
            "epoch={epoch} active={:?} quorum_met={} threshold={threshold}",
            active, satisfied
        );
        signers.rotate_left(1);
    }
}
