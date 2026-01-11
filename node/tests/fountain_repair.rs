#![cfg(feature = "integration-tests")]
use the_block::storage::repair::fountain_repair_roundtrip;

#[test]
fn fountain_recovers_single_loss() {
    // Use 180 KiB to stay under the 255 shard limit (FIELD_SIZE-1 in Reed-Solomon)
    // while still exercising the repair path
    let data = vec![42u8; 180 * 1024];
    let recovered = fountain_repair_roundtrip(&data).expect("repair");
    assert_eq!(recovered, data);
}
