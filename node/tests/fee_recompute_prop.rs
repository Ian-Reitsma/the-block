#![cfg(feature = "python-bindings")]
#![cfg(feature = "integration-tests")]
use crypto_suite::hashing::blake3;
use std::{collections::HashMap, fs};

use foundation_serialization::binary;
use testkit::tb_prop_test;
use the_block::transaction::{TxSignature, TxVersion};
use the_block::{
    fee, Block, Blockchain, ChainDisk, FeeLane, Params, RawTxPayload, SignedTransaction,
    TokenAmount,
};

mod util;
use util::temp::temp_dir;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

tb_prop_test!(prop_migration_recomputes_randomized_fees, |runner| {
    runner
        .add_random_case("fee recompute migration", 12, |rng| {
            init();
            let dir = temp_dir("schema_prop_random");
            fs::create_dir_all(dir.path()).unwrap();
            let blocks = rng.range_usize(1..=4);
            let mut chain = Vec::new();
            let mut total_c = 0u64;
            let mut total_i = 0u64;

            for idx in 0..blocks {
                let cb_c = rng.range_u64(0..=999);
                let cb_i = rng.range_u64(0..=999);
                total_c = total_c.saturating_add(cb_c);
                total_i = total_i.saturating_add(cb_i);
                let coinbase = SignedTransaction {
                    payload: RawTxPayload {
                        from_: "cb".into(),
                        to: "miner".into(),
                        amount_consumer: cb_c,
                        amount_industrial: cb_i,
                        fee: 0,
                        pct_ct: 100,
                        nonce: 0,
                        memo: Vec::new(),
                    },
                    public_key: vec![],
                    #[cfg(feature = "quantum")]
                    dilithium_public_key: Vec::new(),
                    signature: TxSignature {
                        ed25519: Vec::new(),
                        #[cfg(feature = "quantum")]
                        dilithium: Vec::new(),
                    },
                    tip: 0,
                    signer_pubkeys: Vec::new(),
                    aggregate_signature: Vec::new(),
                    threshold: 0,
                    lane: FeeLane::Consumer,
                    version: TxVersion::Ed25519Only,
                };
                let tx_count = rng.range_usize(0..=4);
                let mut txs = vec![coinbase.clone()];
                for n in 0..tx_count {
                    let selector = rng.range_u64(0..=100) as u64;
                    let fee_amt = rng.range_u64(0..=999);
                    let tx = SignedTransaction {
                        payload: RawTxPayload {
                            from_: "a".into(),
                            to: "b".into(),
                            amount_consumer: 0,
                            amount_industrial: 0,
                            fee: fee_amt,
                            pct_ct: selector,
                            nonce: n as u64 + 1,
                            memo: Vec::new(),
                        },
                        public_key: vec![],
                        #[cfg(feature = "quantum")]
                        dilithium_public_key: Vec::new(),
                        signature: TxSignature {
                            ed25519: Vec::new(),
                            #[cfg(feature = "quantum")]
                            dilithium: Vec::new(),
                        },
                        tip: 0,
                        signer_pubkeys: Vec::new(),
                        aggregate_signature: Vec::new(),
                        threshold: 0,
                        lane: FeeLane::Consumer,
                        version: TxVersion::Ed25519Only,
                    };
                    txs.push(tx);
                }
                let block = Block {
                    index: idx as u64,
                    previous_hash: "0".repeat(64),
                    transactions: txs,
                    difficulty: 1,
                    base_fee: 1,
                    ..Block::default()
                };
                chain.push(block);
            }

            let disk = ChainDisk {
                schema_version: 3,
                chain,
                accounts: HashMap::new(),
                emission_consumer: 0,
                emission_industrial: 0,
                emission_consumer_year_ago: 0,
                inflation_epoch_marker: 0,
                block_reward_consumer: TokenAmount::new(0),
                block_reward_industrial: TokenAmount::new(0),
                block_height: blocks as u64,
                mempool: Vec::new(),
                base_fee: 1,
                params: Params::default(),
                epoch_storage_bytes: 0,
                epoch_read_bytes: 0,
                epoch_cpu_ms: 0,
                epoch_bytes_out: 0,
                recent_timestamps: Vec::new(),
            };
            let mut map: HashMap<String, Vec<u8>> = HashMap::new();
            map.insert("chain".to_string(), binary::encode(&disk).unwrap());
            let db_path = dir.path().join("db");
            fs::write(&db_path, binary::encode(&map).unwrap()).unwrap();

            let bc = Blockchain::open(dir.path().to_str().unwrap()).unwrap();
            assert_eq!(bc.circulating_supply(), (total_c, total_i));
            for blk in &bc.chain {
                if let Some(cb) = blk.transactions.first() {
                    assert_eq!(blk.coinbase_consumer.get(), cb.payload.amount_consumer);
                    assert_eq!(blk.coinbase_industrial.get(), cb.payload.amount_industrial);
                }
                let mut fee_c: u128 = 0;
                let mut fee_i: u128 = 0;
                for tx in blk.transactions.iter().skip(1) {
                    if let Ok((c, i)) = fee::decompose(tx.payload.pct_ct, tx.payload.fee) {
                        fee_c += c as u128;
                        fee_i += i as u128;
                    }
                }
                let fc = u64::try_from(fee_c).unwrap_or(0);
                let fi = u64::try_from(fee_i).unwrap_or(0);
                let mut h = blake3::Hasher::new();
                h.update(&fc.to_le_bytes());
                h.update(&fi.to_le_bytes());
                assert_eq!(blk.fee_checksum, h.finalize().to_hex().to_string());
            }
        })
        .expect("register random case");
});
