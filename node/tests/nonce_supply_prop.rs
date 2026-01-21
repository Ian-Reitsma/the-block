#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use testkit::tb_prop_test;
use the_block::{generate_keypair, sign_tx, Blockchain, RawTxPayload, SignedTransaction};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {});
    std::env::set_var("TB_FAST_MINE", "1");
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
        pct: 100,
        nonce,
        memo: Vec::new(),
    };
    // Validate secret key is exactly 32 bytes for ed25519
    let secret: [u8; 32] = sk
        .try_into()
        .expect("secret key must be 32 bytes for ed25519");
    sign_tx(secret.to_vec(), payload).expect("valid key")
}

tb_prop_test!(nonce_and_supply_hold, |runner| {
    runner
        .add_case("empty sequence", || {
            init();
            let dir = temp_dir("nonce_supply_prop_empty");
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            bc.min_fee_per_byte_consumer = 0;
            bc.min_fee_per_byte_industrial = 0;
            bc.recompute_difficulty();
            bc.add_account("a".into(), 100_000).unwrap();
            bc.add_account("b".into(), 0).unwrap();
            let (sk, pk) = generate_keypair();
            // Verify keypair is valid size
            assert_eq!(sk.len(), 32, "Secret key must be 32 bytes");
            assert_eq!(pk.len(), 32, "Public key must be 32 bytes");
            bc.mine_block("a").unwrap();
            let tx = build_signed_tx(&sk, "a", "b", 1, 0, 1000, 1);
            // Verify transaction signature before submitting
            assert!(
                the_block::transaction::verify_signed_tx(&tx),
                "Transaction signature verification failed"
            );
            bc.submit_transaction(tx)
                .unwrap_or_else(|e| panic!("Failed to submit transaction: {:?}", e));
            bc.mine_block("a").unwrap();
            assert_eq!(bc.accounts.get("a").unwrap().nonce, 1);
        })
        .expect("register deterministic case");

    runner
        .add_random_case("nonce/supply sequences", 16, |rng| {
            init();
            let dir = temp_dir("nonce_supply_prop");
            let mut bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 0).unwrap();
            bc.min_fee_per_byte_consumer = 0;
            bc.min_fee_per_byte_industrial = 0;
            bc.recompute_difficulty();
            bc.add_account("a".into(), 100_000).unwrap();
            bc.add_account("b".into(), 0).unwrap();
            let (sk, pk) = generate_keypair();
            // Verify keypair is valid size
            assert_eq!(sk.len(), 32, "Secret key must be 32 bytes");
            assert_eq!(pk.len(), 32, "Public key must be 32 bytes");
            bc.mine_block("a").unwrap();
            let mut expected_nonce = 0u64;
            let rounds = rng.range_usize(1..=8);
            for _ in 0..rounds {
                let consumer = rng.range_u64(0..=50) % 5;
                let industrial = 0; // Single token via consumer lane only
                let tx = build_signed_tx(
                    &sk,
                    "a",
                    "b",
                    consumer,
                    industrial,
                    1000,
                    expected_nonce + 1,
                );
                // Verify transaction signature before submitting
                assert!(
                    the_block::transaction::verify_signed_tx(&tx),
                    "Transaction signature verification failed at nonce {}",
                    expected_nonce + 1
                );
                bc.submit_transaction(tx).unwrap_or_else(|e| {
                    panic!(
                        "Failed to submit transaction at nonce {}: {:?}",
                        expected_nonce + 1,
                        e
                    )
                });
                bc.mine_block("a").unwrap();
                expected_nonce += 1;
                assert_eq!(bc.accounts.get("a").unwrap().nonce, expected_nonce);
                let mut total = 0u64;
                for acc in bc.accounts.values() {
                    total += acc.balance.amount;
                }
                let circulating = bc.circulating_supply();
                assert_eq!(total, circulating);
            }
        })
        .expect("register random case");
});
