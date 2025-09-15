use bridges::{Bridge, BridgeConfig, RelayerBundle, RelayerProof, RelayerSet};

/// Simulate adversarial timing by replaying delayed withdrawals.
pub fn simulate_delay_rounds(rounds: u32) {
    let mut bridge = Bridge::new(BridgeConfig::default());
    let mut relayers = RelayerSet::default();
    let user = "sim-user";
    let amount = 10;
    let proofs = vec![RelayerProof::new("r1", user, amount), RelayerProof::new("r2", user, amount)];
    let bundle = RelayerBundle::new(proofs);
    let mut header = bridges::light_client::Header {
        chain_id: "sim".into(),
        height: 1,
        merkle_root: [0u8; 32],
        signature: [0u8; 32],
    };
    let header_hash = bridges::light_client::header_hash(&header);
    header.signature = header_hash;
    let pow_header = bridges::header::PowHeader {
        chain_id: header.chain_id.clone(),
        height: header.height,
        merkle_root: header.merkle_root,
        signature: header_hash,
        nonce: 1,
        target: u64::MAX,
    };
    let proof = bridges::light_client::Proof {
        leaf: [0u8; 32],
        path: vec![],
    };
    let _ = bridge.deposit_with_relayer(&mut relayers, "r1", user, amount, &pow_header, &proof, &bundle);
    for _ in 0..rounds {
        let _ = bridge.unlock_with_relayer(&mut relayers, "r1", user, amount, &bundle);
    }
}
