#![allow(clippy::unwrap_used, clippy::expect_used)]

use the_block::Blockchain;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

#[test]
fn restore_from_snapshot_and_diffs() {
    init();
    std::env::set_var("TB_SNAPSHOT_INTERVAL", "2");
    let dir = temp_dir("snapdiff_db");
    let accounts_before;
    {
        let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
        bc.mine_block("miner").unwrap();
        bc.mine_block("miner").unwrap(); // full snapshot at height 2
        bc.mine_block("miner").unwrap(); // diff at height 3
        accounts_before = bc.accounts.clone();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(bc2.accounts, accounts_before);
}
