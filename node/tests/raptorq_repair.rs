use the_block::storage::repair::raptorq_repair_roundtrip;

#[test]
fn raptorq_recovers_single_loss() {
    // Use a 256 KiB shard which exercises the repair path while keeping the test
    // comfortably under the harness timeout.
    let data = vec![42u8; 256 * 1024];
    let recovered = raptorq_repair_roundtrip(&data).expect("repair");
    assert_eq!(recovered, data);
}
