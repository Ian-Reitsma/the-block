#![cfg(feature = "integration-tests")]
use the_block::storage::repair::fountain_repair_roundtrip;

#[test]
fn fountain_recovers_single_loss() {
    // Use a 256 KiB shard which exercises the repair path while keeping the test
    // comfortably under the harness timeout.
    let data = vec![42u8; 256 * 1024];
    let recovered = fountain_repair_roundtrip(&data).expect("repair");
    assert_eq!(recovered, data);
}
