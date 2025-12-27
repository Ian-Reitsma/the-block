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
        coinbase_block: 0,
        coinbase_industrial: 0,
        storage_sub_ct: 0,
        read_sub_ct: 0,
        compute_sub_ct: 0,
        proof_rebate_ct: 0,
        storage_sub_it: 0,
        read_sub_it: 0,
        compute_sub_it: 0,
        read_root: [0; 32],
        fee_checksum: String::new(),
        state_root: String::new(),
        base_fee: 0,
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0; 32],
        vdf_output: [0; 32],
        vdf_proof: Vec::new(),
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
