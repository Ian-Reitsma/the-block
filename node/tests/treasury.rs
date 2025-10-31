use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sys::tempfile::tempdir;
use the_block::governance::{
    DisbursementStatus, GovStore, TreasuryBalanceEventKind, NODE_GOV_STORE,
};
use the_block::treasury_executor::{
    memo_dependency_check, spawn_executor as spawn_treasury_executor, ExecutorParams,
};
use the_block::{Account, Blockchain, TokenBalance};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn node_treasury_accrual_flow() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    assert_eq!(store.treasury_balance()?, 0);

    store.record_treasury_accrual(64, 16)?;
    assert_eq!(store.treasury_balance()?, 64);

    let queued = store.queue_disbursement("dest", 10, 4, "", 0)?;
    assert_eq!(queued.id, 1);
    assert_eq!(store.treasury_balance()?, 64);

    let executed = store.execute_disbursement(queued.id, "0xabc")?;
    assert!(matches!(
        executed.status,
        DisbursementStatus::Executed { .. }
    ));
    assert_eq!(store.treasury_balance()?, 54);

    store.cancel_disbursement(queued.id, "noop")?;
    let history = store.treasury_balance_history()?;
    assert!(history
        .iter()
        .any(|snap| matches!(snap.event, TreasuryBalanceEventKind::Executed)));
    assert_eq!(
        history
            .last()
            .map(|snap| (snap.balance_ct, snap.balance_it))
            .unwrap(),
        (54, 12)
    );
    Ok(())
}

#[test]
fn mining_diverts_treasury_share() -> Result<()> {
    let _ = std::fs::remove_dir_all("governance_db");
    let before = NODE_GOV_STORE.treasury_balance()?;
    let start_len = NODE_GOV_STORE.treasury_balance_history()?.len();

    let mut chain = Blockchain::default();
    chain.params.treasury_percent_ct = 25;
    chain
        .mine_block("miner")
        .map_err(|e| format!("mine block: {e}"))?;

    let after = NODE_GOV_STORE.treasury_balance()?;
    let history = NODE_GOV_STORE.treasury_balance_history()?;
    assert!(after > before);
    assert!(history.len() > start_len);
    assert!(matches!(
        history.last().unwrap().event,
        TreasuryBalanceEventKind::Accrual
    ));
    Ok(())
}

#[test]
fn treasury_executor_respects_dependency_schedule() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    store.record_treasury_accrual(2_000, 2_000)?;
    let first = store.queue_disbursement("alpha", 100, 10, "{}", 2)?;
    let second_memo = "{\"depends_on\":[1]}";
    let second = store.queue_disbursement("beta", 120, 5, second_memo, 1)?;
    let blockchain = Arc::new(Mutex::new(Blockchain::default()));
    {
        let mut chain = blockchain.lock().unwrap();
        chain.base_fee = 1;
        chain.block_height = 120; // epoch 1
        chain.config.treasury_account = "treasury".into();
        chain.accounts.insert(
            "treasury".into(),
            Account {
                address: "treasury".into(),
                balance: TokenBalance {
                    consumer: 5_000,
                    industrial: 5_000,
                },
                nonce: 0,
                pending_consumer: 0,
                pending_industrial: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            },
        );
    }
    let (sk, _) = the_block::generate_keypair();
    let params = ExecutorParams {
        identity: "node-test-exec".into(),
        poll_interval: Duration::from_millis(50),
        lease_ttl: Duration::from_millis(200),
        signing_key: Arc::new(sk),
        treasury_account: "treasury".into(),
        dependency_check: Some(memo_dependency_check()),
    };
    let handle = spawn_treasury_executor(&store, Arc::clone(&blockchain), params);
    thread::sleep(Duration::from_millis(200));
    let pending = store.disbursements()?;
    let first_status = pending
        .iter()
        .find(|d| d.id == first.id)
        .map(|d| d.status.clone())
        .expect("first disbursement present");
    let second_status = pending
        .iter()
        .find(|d| d.id == second.id)
        .map(|d| d.status.clone())
        .expect("second disbursement present");
    assert!(matches!(first_status, DisbursementStatus::Scheduled));
    assert!(matches!(second_status, DisbursementStatus::Scheduled));
    {
        let mut chain = blockchain.lock().unwrap();
        chain.block_height = 240; // epoch 2
    }
    assert_eq!(blockchain.lock().unwrap().block_height, 240);
    let mut first_final = None;
    let mut second_final = None;
    for _ in 0..100 {
        let intents_empty = store.execution_intents()?.is_empty();
        let final_state = store.disbursements()?;
        let first_candidate = final_state.iter().find(|d| d.id == first.id).cloned();
        let second_candidate = final_state.iter().find(|d| d.id == second.id).cloned();
        if let (Some(f), Some(s)) = (first_candidate, second_candidate) {
            if matches!(f.status, DisbursementStatus::Executed { .. })
                && matches!(s.status, DisbursementStatus::Executed { .. })
                && intents_empty
            {
                first_final = Some(f);
                second_final = Some(s);
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    handle.shutdown();
    handle.join();
    if first_final.is_none() || second_final.is_none() {
        let reader = GovStore::open(&db_path);
        let final_state = reader.disbursements()?;
        first_final = final_state.iter().find(|d| d.id == first.id).cloned();
        second_final = final_state.iter().find(|d| d.id == second.id).cloned();
    }
    let first_final = first_final.expect("treasury executor did not execute first disbursement");
    let second_final = second_final.expect("treasury executor did not execute second disbursement");
    assert!(matches!(
        first_final.status,
        DisbursementStatus::Executed { .. }
    ));
    assert!(matches!(
        second_final.status,
        DisbursementStatus::Executed { .. }
    ));
    Ok(())
}
