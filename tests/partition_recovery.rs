use the_block::net::partition_watch::PartitionWatch;
use the_block::{net, partition_recover, Block, Blockchain};
#[cfg(feature = "telemetry")]
use the_block::telemetry::{PARTITION_EVENTS_TOTAL, PARTITION_RECOVER_BLOCKS};

fn dummy_block() -> Block {
    Block {
        index: 0,
        previous_hash: String::new(),
        timestamp_millis: 0,
        transactions: Vec::new(),
        difficulty: 0,
        retune_hint: 0,
        nonce: 0,
        hash: String::new(),
        coinbase_block: 0.into(),
        coinbase_industrial: 0.into(),
        storage_sub: 0.into(),
        read_sub: 0.into(),
        read_sub_viewer: 0.into(),
        read_sub_host: 0.into(),
        read_sub_hardware: 0.into(),
        read_sub_verifier: 0.into(),
        read_sub_liquidity: 0.into(),
        ad_viewer: 0.into(),
        ad_host: 0.into(),
        ad_hardware: 0.into(),
        ad_verifier: 0.into(),
        ad_liquidity: 0.into(),
        ad_miner: 0.into(),
        treasury_events: Vec::new(),
        ad_total_usd_micros: 0,
        ad_settlement_count: 0,
        ad_oracle_price_usd_micros: 0,
        compute_sub: 0.into(),
        proof_rebate: 0.into(),
        read_root: [0; 32],
        fee_checksum: String::new(),
        state_root: String::new(),
        root_bundles: Vec::new(),
        base_fee: 0,
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0; 32],
        vdf_output: [0; 32],
        vdf_proof: Vec::new(),
        receipts: Vec::new(),
        #[cfg(feature = "quantum")]
        dilithium_pubkey: Vec::new(),
        #[cfg(feature = "quantum")]
        dilithium_sig: Vec::new(),
    }
}

#[test]
fn partition_detection_and_recovery_metrics() {
    let watch = PartitionWatch::new(1);
    let peer_bytes = [1u8; 32];
    let peer = net::overlay_peer_from_bytes(&peer_bytes).expect("overlay peer");
    watch.mark_unreachable(peer.clone());
    assert!(watch.is_partitioned());
    #[cfg(feature = "telemetry")]
    assert_eq!(PARTITION_EVENTS_TOTAL.get(), 1);
    let mut chain = the_block::Blockchain::default();
    let blocks = vec![dummy_block(), dummy_block()];
    partition_recover::replay_blocks(&mut chain, &blocks);
    #[cfg(feature = "telemetry")]
    assert_eq!(PARTITION_RECOVER_BLOCKS.get(), 2);
}
