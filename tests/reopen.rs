#![allow(clippy::unwrap_used, clippy::expect_used)]

use base64::Engine;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{
    generate_keypair, sign_tx, Account, Blockchain, ChainDisk, MempoolEntryDisk, Pending,
    RawTxPayload, TokenAmount, TokenBalance, TxAdmissionError,
};

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

fn unique_path(prefix: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let id = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{id}")
}

fn load_fixture(name: &str) -> String {
    let dir = unique_path("chain_db");
    fs::create_dir_all(&dir).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .join("db.b64");
    let b64 = fs::read_to_string(src).unwrap();
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean)
        .unwrap();
    let dst = Path::new(&dir).join("db");
    fs::write(&dst, bytes).unwrap();
    dir
}

#[test]
fn open_mine_reopen() {
    init();
    let (priv_a, _) = generate_keypair();
    let path = unique_path("chain_db");
    let _ = fs::remove_dir_all(&path);

    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        // Keep the database directory for the reopen but close handles cleanly.
        bc.path.clear();
    }

    let mut bc = Blockchain::open(&path).unwrap();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(priv_a.to_vec(), payload).unwrap();
    assert!(bc.submit_transaction(tx).is_ok());
}

#[test]
fn replay_after_crash_is_duplicate() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("replay_db");
    let _ = fs::remove_dir_all(&path);
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let mut bc2 = Blockchain::open(&path).unwrap();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    assert_eq!(bc2.submit_transaction(tx), Err(TxAdmissionError::Duplicate));
}

#[test]
fn ttl_expired_purged_on_restart() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("replay_ttl");
    let _ = fs::remove_dir_all(&path);
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.tx_ttl = 1;
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
            entry.timestamp_millis = 0;
            entry.timestamp_ticks = 0;
        }
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(&path).unwrap();
    assert!(bc2.mempool.is_empty());
}

#[test]
fn startup_ttl_purge_increments_metrics() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("startup_ttl_metrics");
    let _ = fs::remove_dir_all(&path);
    #[cfg(feature = "telemetry")]
    {
        telemetry::TTL_DROP_TOTAL.reset();
        telemetry::STARTUP_TTL_DROP_TOTAL.reset();
        telemetry::MEMPOOL_SIZE.set(0);
    }
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.tx_ttl = 1;
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        if let Some(mut entry) = bc.mempool.get_mut(&("a".into(), 1)) {
            entry.timestamp_millis = 0;
            entry.timestamp_ticks = 0;
        }
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(&path).unwrap();
    assert_eq!(0, bc2.mempool.len());
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(1, telemetry::TTL_DROP_TOTAL.get());
        assert_eq!(1, telemetry::STARTUP_TTL_DROP_TOTAL.get());
        assert_eq!(0, telemetry::MEMPOOL_SIZE.get());
    }
}

#[test]
fn startup_missing_account_does_not_increment_startup_ttl_drop_total() {
    init();
    let path = unique_path("startup_orphan_metrics");
    let _ = fs::remove_dir_all(&path);
    #[cfg(feature = "telemetry")]
    {
        telemetry::STARTUP_TTL_DROP_TOTAL.reset();
        telemetry::ORPHAN_SWEEP_TOTAL.reset();
        telemetry::MEMPOOL_SIZE.set(0);
    }
    {
        let (sk, _pk) = generate_keypair();
        fs::create_dir_all(&path).unwrap();
        let payload = RawTxPayload {
            from_: "ghost".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let entry = MempoolEntryDisk {
            sender: "ghost".into(),
            nonce: 1,
            tx: tx.clone(),
            timestamp_millis: now,
            timestamp_ticks: now,
        };
        let disk = ChainDisk {
            schema_version: 4,
            chain: Vec::new(),
            accounts: HashMap::new(),
            emission_consumer: 0,
            emission_industrial: 0,
            block_reward_consumer: TokenAmount::new(0),
            block_reward_industrial: TokenAmount::new(0),
            block_height: 0,
            mempool: vec![entry],
        };
        let mut map: HashMap<String, Vec<u8>> = HashMap::new();
        map.insert("chain".to_string(), bincode::serialize(&disk).unwrap());
        let db_path = Path::new(&path).join("db");
        fs::write(db_path, bincode::serialize(&map).unwrap()).unwrap();
    }
    let bc = Blockchain::open(&path).unwrap();
    assert!(bc.mempool.is_empty());
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(0, telemetry::STARTUP_TTL_DROP_TOTAL.get());
        assert_eq!(1, telemetry::ORPHAN_SWEEP_TOTAL.get());
    }
}

#[test]
fn timestamp_ticks_persist_across_restart() {
    init();
    let (sk, _pk) = generate_keypair();
    let path = unique_path("ticks_db");
    let _ = fs::remove_dir_all(&path);
    let first;
    {
        let mut bc = Blockchain::open(&path).unwrap();
        bc.add_account("a".into(), 0, 0).unwrap();
        bc.add_account("b".into(), 0, 0).unwrap();
        bc.mine_block("a").unwrap();
        let payload = RawTxPayload {
            from_: "a".into(),
            to: "b".into(),
            amount_consumer: 1,
            amount_industrial: 1,
            fee: 1000,
            fee_selector: 0,
            nonce: 1,
            memo: Vec::new(),
        };
        let tx = sign_tx(sk.to_vec(), payload).unwrap();
        bc.submit_transaction(tx).unwrap();
        first = bc
            .mempool
            .get(&("a".into(), 1))
            .map(|e| e.timestamp_ticks)
            .unwrap();
        bc.persist_chain().unwrap();
        bc.path.clear();
    }
    let bc2 = Blockchain::open(&path).unwrap();
    let persisted = bc2
        .mempool
        .get(&("a".into(), 1))
        .map(|e| e.timestamp_ticks)
        .unwrap();
    assert_eq!(first, persisted);
}

#[test]
fn schema_upgrade_compatibility() {
    init();
    for fixture in ["v1", "v2"] {
        let path = load_fixture(fixture);
        let bc = Blockchain::open(&path).unwrap();
        for acc in bc.accounts.values() {
            assert_eq!(acc.pending.consumer, 0);
            assert_eq!(acc.pending.industrial, 0);
            assert_eq!(acc.pending.nonce, 0);
        }
    }

    let path = unique_path("schema_v3");
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "a".into(),
        to: "b".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1000,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let entry = MempoolEntryDisk {
        sender: "a".into(),
        nonce: 1,
        tx: tx.clone(),
        timestamp_millis: now,
        timestamp_ticks: 0,
    };
    let mut accounts = HashMap::new();
    accounts.insert(
        "a".into(),
        Account {
            address: "a".into(),
            balance: TokenBalance {
                consumer: 10,
                industrial: 10,
            },
            nonce: 0,
            pending: Pending::default(),
        },
    );
    let disk = ChainDisk {
        schema_version: 3,
        chain: Vec::new(),
        accounts,
        emission_consumer: 0,
        emission_industrial: 0,
        block_reward_consumer: TokenAmount::new(0),
        block_reward_industrial: TokenAmount::new(0),
        block_height: 0,
        mempool: vec![entry],
    };
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    map.insert("chain".to_string(), bincode::serialize(&disk).unwrap());
    let db_path = Path::new(&path).join("db");
    fs::write(db_path, bincode::serialize(&map).unwrap()).unwrap();

    let bc = Blockchain::open(&path).unwrap();
    let migrated = bc.mempool.get(&(String::from("a"), 1)).unwrap();
    assert_eq!(migrated.timestamp_ticks, migrated.timestamp_millis);
}
