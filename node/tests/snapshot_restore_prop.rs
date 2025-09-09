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
    #![proptest_config(ProptestConfig { cases: 8, failure_persistence: None, .. ProptestConfig::default() })]
    #[test]
    #[ignore]
    fn snapshot_restore_roundtrip(blocks in 1u64..6) {
        init();
        std::env::set_var("TB_SNAPSHOT_INTERVAL", "2");
        let dir = temp_dir("snap_prop");
        let accounts;
        {
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            bc.recompute_difficulty();
            for _ in 0..blocks {
                bc.mine_block("miner").unwrap();
            }
            accounts = bc.accounts.clone();
            bc.path.clear();
        }
        let mut bc2 = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
        bc2.recompute_difficulty();
        prop_assert_eq!(bc2.accounts.clone(), accounts);
    }
}
