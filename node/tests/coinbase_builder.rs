use serial_test::serial;
use the_block::Blockchain;

mod util;

#[test]
#[serial]
fn coinbase_tip_defaults_to_zero() {
    let dir = util::temp::temp_dir("coinbase_tip");
    let mut bc = Blockchain::new(dir.path().to_str().expect("path"));
    bc.add_account("miner".into(), 0, 0).expect("add miner");

    let block = bc.mine_block("miner").expect("mine block");
    assert_eq!(block.transactions[0].tip, 0);
}
