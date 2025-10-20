use sys::tempfile::tempdir;
use the_block::governance::{
    DisbursementStatus, GovStore, TreasuryBalanceEventKind, NODE_GOV_STORE,
};
use the_block::Blockchain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn node_treasury_accrual_flow() -> Result<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");
    let store = GovStore::open(&db_path);
    assert_eq!(store.treasury_balance()?, 0);

    store.record_treasury_accrual(64)?;
    assert_eq!(store.treasury_balance()?, 64);

    let queued = store.queue_disbursement("dest", 10, "", 0)?;
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
