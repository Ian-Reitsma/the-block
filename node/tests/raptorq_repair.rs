use the_block::storage::repair::raptorq_repair_roundtrip;

#[test]
fn raptorq_recovers_single_loss() {
    // 4 MiB shard to match overlay chunk size
    let data = vec![42u8; 4 * 1024 * 1024];
    let recovered = raptorq_repair_roundtrip(&data).expect("repair");
    assert_eq!(recovered, data);
}
