use std::cmp::Ordering;
use the_block::consensus::fork_choice::{choose_tip, TipMeta};

#[test]
fn prefers_higher_checkpoint_then_height() {
    let a = TipMeta {
        height: 5,
        weight: 100,
        tip_hash: [1u8; 32],
        checkpoint_height: 10,
        rollback: None,
    };
    let b = TipMeta {
        height: 6,
        weight: 110,
        tip_hash: [2u8; 32],
        checkpoint_height: 8,
        rollback: None,
    };
    // Despite lower PoW height, chain A wins due to higher PoS checkpoint
    assert_eq!(Ordering::Greater, choose_tip(&a, &b));
}

#[test]
fn falls_back_to_weight_and_hash() {
    let a = TipMeta {
        height: 5,
        weight: 100,
        tip_hash: [1u8; 32],
        checkpoint_height: 10,
        rollback: None,
    };
    let b = TipMeta {
        height: 5,
        weight: 90,
        tip_hash: [2u8; 32],
        checkpoint_height: 10,
        rollback: None,
    };
    assert_eq!(Ordering::Greater, choose_tip(&a, &b));
}
