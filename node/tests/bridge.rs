#![allow(clippy::unwrap_used, clippy::expect_used)]
use the_block::bridge::Bridge;

#[test]
fn round_trip_transfer() {
    let mut src = Bridge::default();
    let mut dst = Bridge::default();
    src.lock("alice", 100);
    assert_eq!(src.locked("alice"), 100);
    // relayer mints on destination
    dst.mint("alice", 100);
    assert_eq!(dst.minted("alice"), 100);
    // burn and release
    assert!(dst.burn("alice", 100));
    assert!(src.release("alice", 100));
    assert_eq!(src.locked("alice"), 0);
}
