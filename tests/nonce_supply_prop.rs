use proptest::prelude::*;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1))]
    #[test]
    fn nonce_and_supply_hold(seq in proptest::collection::vec((0u64..20, 0u64..20), 1..3)) {
        init();
        let dir = temp_dir("nonce_supply_prop");
        let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        let (sk, _pk) = generate_keypair();
        // initial funding
        bc.mine_block("a").unwrap();
        let mut expected_nonce = 0u64;
        for (c,i) in seq {
            let payload = RawTxPayload {
                from_: "a".into(),
                to: "b".into(),
                amount_consumer: c % 5,
                amount_industrial: i % 5,
                fee: 1000,
                fee_selector: 0,
                nonce: expected_nonce + 1,
                memo: Vec::new(),
            };
            let tx = sign_tx(sk.clone(), payload).unwrap();
            bc.submit_transaction(tx).unwrap();
            bc.mine_block("a").unwrap();
            expected_nonce += 1;
            assert_eq!(bc.accounts.get("a").unwrap().nonce, expected_nonce);
            let mut total_c = 0u64;
            let mut total_i = 0u64;
            for acc in bc.accounts.values() {
                total_c += acc.balance.consumer;
                total_i += acc.balance.industrial;
            }
            let (em_c, em_i) = bc.circulating_supply();
            assert_eq!(total_c, em_c);
            assert_eq!(total_i, em_i);
        }
    }
}
