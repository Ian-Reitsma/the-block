#![allow(clippy::unwrap_used, clippy::expect_used)]
use proptest::prelude::*;
use the_block::Blockchain;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

proptest! {
    #[test]
    fn snapshot_restore_roundtrip(blocks in 1u64..6) {
        init();
        std::env::set_var("TB_SNAPSHOT_INTERVAL", "2");
        let dir = temp_dir("snap_prop");
        let accounts;
        {
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            for _ in 0..blocks {
                bc.mine_block("miner").unwrap();
            }
            accounts = bc.accounts.clone();
            bc.path.clear();
        }
        let bc2 = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
        prop_assert_eq!(bc2.accounts.clone(), accounts);
    }
}
