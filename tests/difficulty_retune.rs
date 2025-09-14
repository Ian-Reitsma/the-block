use the_block::Blockchain;

#[test]
fn retune_handles_spikes() {
    let mut bc = Blockchain::default();
    bc.add_account("miner".into(), 0, 0).unwrap();
    // fast blocks
    for i in 0..5 {
        bc.mine_block_at("miner", i * 100).unwrap();
    }
    let up = bc.difficulty;
    // slow blocks
    for i in 0..5 {
        bc.mine_block_at("miner", 1000 + i * 2000).unwrap();
    }
    let down = bc.difficulty;
    assert!(up > down);
}

#[test]
fn deterministic_across_nodes() {
    let mut a = Blockchain::default();
    let mut b = Blockchain::default();
    a.add_account("m".into(), 0, 0).unwrap();
    b.add_account("m".into(), 0, 0).unwrap();
    for i in 0..6 {
        let ts = 1000 + i * 1000;
        a.mine_block_at("m", ts).unwrap();
        b.mine_block_at("m", ts).unwrap();
    }
    assert_eq!(a.difficulty, b.difficulty);
    assert_eq!(a.retune_hint, b.retune_hint);
}
