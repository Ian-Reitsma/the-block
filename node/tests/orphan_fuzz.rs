use std::fs;
use std::sync::{Arc, RwLock};

use proptest::prelude::*;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

mod util;
use util::temp::temp_dir;

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn build_signed_tx(
    sk: &[u8],
    from: &str,
    to: &str,
    consumer: u64,
    industrial: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransaction {
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer: consumer,
        amount_industrial: industrial,
        fee,
        fee_selector: 0,
        nonce,
        memo: Vec::new(),
    };
    sign_tx(sk.to_vec(), payload).expect("valid key")
}

#[derive(Clone, Debug)]
enum Op {
    Remove(usize),
    Purge,
}

const ACCOUNTS: usize = 8;

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, failure_persistence: None, .. ProptestConfig::default() })]
    #[test]
    fn orphan_counter_never_exceeds_mempool(
        ops in prop::collection::vec(
            prop_oneof![
                (0usize..ACCOUNTS).prop_map(Op::Remove),
                Just(Op::Purge)
            ],
            1..20
        )
    ) {
        init();
        let dir = temp_dir("temp_orphan_fuzz");
        let mut bc = Blockchain::new(dir.path().to_str().unwrap());
        bc.min_fee_per_byte_consumer = 0;
        bc.min_fee_per_byte_industrial = 0;
        bc.add_account("sink".into(), 0, 0).unwrap();
        for i in 0..ACCOUNTS {
            let name = format!("acc{i}");
            bc.add_account(name.clone(), 1_000_000, 0).unwrap();
            let (sk, _pk) = generate_keypair();
            let tx = build_signed_tx(&sk, &name, "sink", 1, 0, 1_000, 1);
            bc.submit_transaction(tx).unwrap();
        }
        for mut entry in bc.mempool_consumer.iter_mut() {
            entry.value_mut().timestamp_millis = 0;
        }
        let bc = Arc::new(RwLock::new(bc));
        let handles: Vec<_> = ops.into_iter().map(|op| {
            let bc_cl = Arc::clone(&bc);
            std::thread::spawn(move || match op {
                Op::Remove(idx) => {
                    let key = format!("acc{idx}");
                    bc_cl.write().unwrap().accounts.remove(&key);
                }
                Op::Purge => {
                    let _ = bc_cl.write().unwrap().purge_expired();
                }
            })
        }).collect();
        for h in handles {
            h.join().unwrap();
        }
        let guard = bc.read().unwrap();
        let orphans = guard.orphan_count();
        let size = guard.mempool_consumer.len();
        assert_ne!(orphans, usize::MAX);
        assert!(orphans <= size);
    }
}
