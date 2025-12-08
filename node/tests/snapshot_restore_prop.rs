#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use testkit::tb_prop_test;
use the_block::Blockchain;

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
    });
}

tb_prop_test!(snapshot_restore_roundtrip, |runner| {
    runner
        .add_random_case("snapshot roundtrip", 16, |rng| {
            init();
            std::env::set_var("TB_SNAPSHOT_INTERVAL", "2");
            let dir = temp_dir("snap_prop");
            let blocks = rng.range_u64(1..=5);
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
            assert_eq!(bc2.accounts.clone(), accounts);
        })
        .expect("register random case");
});
