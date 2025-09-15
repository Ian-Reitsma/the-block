use the_block::Blockchain;

#[test]
fn emits_macro_block() {
    let mut bc = Blockchain::new("state/macro_block_test");
    bc.macro_interval = 2;
    bc.difficulty = 0;
    bc.mine_block_at("miner", 1).unwrap();
    bc.mine_block_at("miner", 2).unwrap();
    assert_eq!(bc.macro_blocks.len(), 1);
    let mb = &bc.macro_blocks[0];
    assert_eq!(mb.height, 2);
    assert_eq!(mb.reward_consumer, bc.block_reward_consumer.0 * 2);
}
