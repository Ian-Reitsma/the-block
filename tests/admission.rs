use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use the_block::hashlayout::BlockEncoder;
use the_block::{
    generate_keypair, sign_tx, Blockchain, MempoolEntry, RawTxPayload, SignedTransaction,
    TokenAmount, TxAdmissionError,
};

fn init() {
    let _ = fs::remove_dir_all("chain_db");
    pyo3::prepare_freethreaded_python();
}

fn unique_path(prefix: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{id}")
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

#[test]
fn rejects_unknown_sender() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_admission"));
    bc.add_account("miner".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "miner", 1, 0, 1000, 1);
    assert!(bc.submit_transaction(tx).is_err());
}

#[test]
fn mine_block_skips_nonce_gaps() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_admission"));
    bc.add_account("miner".into(), 10, 10).unwrap();
    bc.add_account("alice".into(), 0, 0).unwrap();
    bc.mine_block("miner").unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "miner", "alice", 1, 1, 1000, 5);
    bc.mempool.insert(
        ("miner".into(), 5),
        MempoolEntry {
            tx: tx.clone(),
            timestamp: Instant::now(),
        },
    );
    let block = bc.mine_block("miner").unwrap();
    assert_eq!(block.transactions.len(), 1); // only coinbase
    assert_eq!(bc.skipped.len(), 1);
    assert_eq!(bc.skipped[0].payload.nonce, 5);
}

#[test]
fn validate_block_rejects_nonce_gap() {
    init();
    let bc = Blockchain::new(&unique_path("temp_admission"));
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "miner", "alice", 0, 0, 1000, 1);
    let tx3 = build_signed_tx(&sk, "miner", "alice", 0, 0, 1000, 3);
    let index = 0u64;
    let prev = "0".repeat(64);
    let diff = the_block::blockchain::difficulty::expected_difficulty(index, bc.difficulty);
    let reward_c = bc.block_reward_consumer.0;
    let reward_i = bc.block_reward_industrial.0;
    let fee_checksum = {
        let mut h = blake3::Hasher::new();
        h.update(&0u64.to_le_bytes());
        h.update(&0u64.to_le_bytes());
        h.finalize().to_hex().to_string()
    };
    let coinbase = SignedTransaction {
        payload: RawTxPayload {
            from_: "0".repeat(34),
            to: "miner".into(),
            amount_consumer: reward_c,
            amount_industrial: reward_i,
            fee: 0,
            fee_selector: 0,
            nonce: 0,
            memo: Vec::new(),
        },
        public_key: vec![],
        signature: vec![],
    };
    let txs = vec![coinbase, tx1.clone(), tx3.clone()];
    let ids: Vec<[u8; 32]> = txs.iter().map(SignedTransaction::id).collect();
    let id_refs: Vec<&[u8]> = ids.iter().map(|h| h.as_ref()).collect();
    let mut nonce = 0u64;
    let hash = loop {
        let enc = BlockEncoder {
            index,
            prev: &prev,
            nonce,
            difficulty: diff,
            coin_c: reward_c,
            coin_i: reward_i,
            fee_checksum: &fee_checksum,
            tx_ids: &id_refs,
        };
        let h = enc.hash();
        let bytes: Vec<u8> = (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect();
        let mut count = 0u32;
        for b in &bytes {
            if *b == 0 {
                count += 8;
            } else {
                count += b.leading_zeros();
                break;
            }
        }
        if count >= diff as u32 {
            break h;
        }
        nonce += 1;
    };
    let block = the_block::Block {
        index,
        previous_hash: prev,
        transactions: txs,
        difficulty: diff,
        nonce,
        hash,
        coinbase_consumer: TokenAmount::new(reward_c),
        coinbase_industrial: TokenAmount::new(reward_i),
        fee_checksum,
    };
    assert!(!bc.validate_block(&block).unwrap());
}

#[test]
fn rejects_fee_below_min() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_fee"));
    bc.add_account("a".into(), 10_000, 0).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "a", "b", 1, 0, 0, 1);
    assert_eq!(bc.submit_transaction(tx), Err(TxAdmissionError::FeeTooLow));
}

#[test]
fn mempool_full_rejects() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_full"));
    bc.max_mempool_size = 1;
    bc.add_account("a".into(), 10_000, 0).unwrap();
    bc.add_account("b".into(), 10_000, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "a", "b", 1, 0, 1000, 1);
    let tx2 = build_signed_tx(&sk, "a", "b", 1, 0, 1000, 2);
    bc.submit_transaction(tx1).unwrap();
    assert_eq!(
        bc.submit_transaction(tx2),
        Err(TxAdmissionError::MempoolFull)
    );
}

#[test]
fn fee_per_byte_boundary() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_fpb"));
    bc.add_account("a".into(), 10_000, 0).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    bc.min_fee_per_byte = 5;
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 0,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx_tmp = sign_tx(sk.clone(), payload.clone()).unwrap();
    let size = bincode::serialize(&tx_tmp).unwrap().len() as u64;
    let mut low = payload.clone();
    low.fee = size * bc.min_fee_per_byte - 1;
    let tx_low = sign_tx(sk.clone(), low).unwrap();
    assert_eq!(
        bc.submit_transaction(tx_low),
        Err(TxAdmissionError::FeeTooLow)
    );
    let mut ok = payload;
    ok.fee = size * bc.min_fee_per_byte;
    let tx_ok = sign_tx(sk, ok).unwrap();
    assert_eq!(bc.submit_transaction(tx_ok), Ok(()));
}

#[test]
fn lock_poisoned_error_and_recovery() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_poison"));
    bc.add_account("alice".into(), 10_000, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx = build_signed_tx(&sk, "alice", "bob", 1, 0, 1000, 1);
    bc.poison_lock("alice");
    assert_eq!(
        bc.submit_transaction(tx.clone()),
        Err(TxAdmissionError::LockPoisoned)
    );
    bc.heal_lock("alice");
    assert_eq!(bc.submit_transaction(tx), Ok(()));
}

#[test]
fn enforces_per_account_pending_limit() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_pending"));
    bc.max_pending_per_account = 1;
    bc.add_account("a".into(), 10_000, 0).unwrap();
    bc.add_account("b".into(), 0, 0).unwrap();
    let (sk, _pk) = generate_keypair();
    let tx1 = build_signed_tx(&sk, "a", "b", 1, 0, 1000, 1);
    let tx2 = build_signed_tx(&sk, "a", "b", 1, 0, 1000, 2);
    assert!(bc.submit_transaction(tx1).is_ok());
    assert_eq!(
        bc.submit_transaction(tx2),
        Err(TxAdmissionError::PendingLimitReached)
    );
}

#[test]
fn validate_block_rejects_wrong_difficulty() {
    init();
    let mut bc = Blockchain::new(&unique_path("temp_admission"));
    bc.add_account("miner".into(), 0, 0).unwrap();
    let mut block = bc.mine_block("miner").unwrap();
    block.difficulty += 1;
    let ids: Vec<[u8; 32]> = block
        .transactions
        .iter()
        .map(SignedTransaction::id)
        .collect();
    let id_refs: Vec<&[u8]> = ids.iter().map(|h| h.as_ref()).collect();
    let enc = BlockEncoder {
        index: block.index,
        prev: &block.previous_hash,
        nonce: block.nonce,
        difficulty: block.difficulty,
        coin_c: block.coinbase_consumer.0,
        coin_i: block.coinbase_industrial.0,
        fee_checksum: &block.fee_checksum,
        tx_ids: &id_refs,
    };
    block.hash = enc.hash();
    assert!(!bc.validate_block(&block).unwrap());
}
