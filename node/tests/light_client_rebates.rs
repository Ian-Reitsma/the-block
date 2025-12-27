#![cfg(feature = "integration-tests")]

use the_block::{light_client::proof_tracker::ProofTracker, Blockchain, TokenAmount};

mod util;

struct PreserveGuard;

impl Drop for PreserveGuard {
    fn drop(&mut self) {
        std::env::remove_var("TB_PRESERVE");
    }
}

#[testkit::tb_serial]
fn rebate_persistence_across_restart() {
    std::env::set_var("TB_PRESERVE", "1");
    let _reset = PreserveGuard;
    let dir = util::temp::temp_dir("rebate_restart");
    let path = dir.path().to_str().expect("path");
    {
        let mut bc = Blockchain::new(path);
        bc.add_account("miner".into(), 0, 0).expect("add miner");
        bc.record_proof_relay(b"relay", 4);
        let block = bc.mine_block("miner").expect("mine block");
        assert_eq!(block.proof_rebate, TokenAmount::new(4));
        let history = bc.proof_tracker.receipt_history(None, None, 8);
        assert_eq!(history.receipts.len(), 1);
        assert_eq!(history.receipts[0].amount, 4);
    }

    let reopened = Blockchain::open(path).expect("reopen blockchain");
    let history = reopened.proof_tracker.receipt_history(None, None, 8);
    assert_eq!(history.receipts.len(), 1);
    let receipt = &history.receipts[0];
    assert_eq!(receipt.amount, 4);
    assert_eq!(receipt.relayers.len(), 1);
    assert_eq!(receipt.relayers[0].id, b"relay".to_vec());
    assert_eq!(receipt.relayers[0].amount, 4);
}

#[testkit::tb_serial]
fn rebate_rollback_restores_pending_balances() {
    let dir = util::temp::temp_dir("rebate_reorg_state");
    let path = dir.path().to_str().expect("path");
    let mut bc = Blockchain::new(path);
    bc.add_account("miner".into(), 0, 0).expect("add miner");
    bc.record_proof_relay(b"relay", 6);
    let block = bc.mine_block("miner").expect("mine block");
    assert_eq!(block.proof_rebate, TokenAmount::new(6));
    let restored = bc.proof_tracker.rollback_claim(block.index);
    assert_eq!(restored, 6);
    let snapshot = bc.proof_tracker.snapshot();
    assert_eq!(snapshot.pending_total, 6);
    let (_id, info) = snapshot
        .relayers
        .iter()
        .find(|(id, _)| id == b"relay".as_ref())
        .expect("relayer tracked");
    assert_eq!(info.pending, 6);
    assert_eq!(info.total_claimed, 0);
    let history = bc.proof_tracker.receipt_history(None, None, 4);
    assert!(history.receipts.is_empty());
}

#[testkit::tb_serial]
fn rebate_double_claim_rejected_after_restart() {
    let dir = util::temp::temp_dir("rebate_double_claim");
    let path = dir.path().join("rebates");
    {
        let mut tracker = ProofTracker::open(&path);
        tracker.record(b"relay", 1, 8);
        assert_eq!(tracker.claim_all(42), 8);
        assert_eq!(tracker.claim_all(42), 0);
    }
    let mut reopened = ProofTracker::open(&path);
    assert_eq!(reopened.claim_all(42), 0);
    let history = reopened.receipt_history(None, None, 4);
    assert_eq!(history.receipts.len(), 1);
    assert_eq!(history.receipts[0].amount, 8);
    let restored = reopened.rollback_claim(42);
    assert_eq!(restored, 8);
    let after_history = reopened.receipt_history(None, None, 4);
    assert!(after_history.receipts.is_empty());
}
