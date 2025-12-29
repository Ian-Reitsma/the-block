#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use testkit::tb_prop_test;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {});
}

tb_prop_test!(nonce_and_supply_hold, |runner| {
    runner
        .add_case("empty sequence", || {
            init();
            let dir = temp_dir("nonce_supply_prop_empty");
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            bc.recompute_difficulty();
            bc.add_account("a".into(), 0, 0).unwrap();
            bc.add_account("b".into(), 0, 0).unwrap();
            let (sk, _pk) = generate_keypair();
            bc.mine_block("a").unwrap();
            let payload = RawTxPayload {
                from_: "a".into(),
                to: "b".into(),
                amount_consumer: 1,
                amount_industrial: 0,  // Single token via consumer lane only
                fee: 1000,
                pct: 100,
                nonce: 1,
                memo: Vec::new(),
            };
            let tx = sign_tx(sk.clone(), payload).unwrap();
            bc.submit_transaction(tx).unwrap();
            bc.mine_block("a").unwrap();
            assert_eq!(bc.accounts.get("a").unwrap().nonce, 1);
        })
        .expect("register deterministic case");

    runner
        .add_random_case("nonce/supply sequences", 16, |rng| {
            init();
            let dir = temp_dir("nonce_supply_prop");
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            bc.recompute_difficulty();
            bc.add_account("a".into(), 0, 0).unwrap();
            bc.add_account("b".into(), 0, 0).unwrap();
            let (sk, _pk) = generate_keypair();
            bc.mine_block("a").unwrap();
            let mut expected_nonce = 0u64;
            let rounds = rng.range_usize(1..=8);
            for _ in 0..rounds {
                let consumer = rng.range_u64(0..=50) % 5;
                let industrial = 0;  // Single token via consumer lane only
                let payload = RawTxPayload {
                    from_: "a".into(),
                    to: "b".into(),
                    amount_consumer: consumer,
                    amount_industrial: industrial,
                    fee: 1000,
                    pct: 100,
                    nonce: expected_nonce + 1,
                    memo: Vec::new(),
                };
                let tx = sign_tx(sk.clone(), payload).unwrap();
                bc.submit_transaction(tx).unwrap();
                bc.mine_block("a").unwrap();
                expected_nonce += 1;
                assert_eq!(bc.accounts.get("a").unwrap().nonce, expected_nonce);
                let mut total_c = 0u64;
                let mut _total_i = 0u64;
                for acc in bc.accounts.values() {
                    total_c += acc.balance.consumer;
                    _total_i += acc.balance.industrial;
                }
                let em_c = bc.circulating_supply();
                assert_eq!(total_c, em_c);
            }
        })
        .expect("register random case");
});
