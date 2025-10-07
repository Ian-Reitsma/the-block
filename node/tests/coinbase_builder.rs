#![cfg(feature = "integration-tests")]
use the_block::{Blockchain, TokenAmount};

mod util;

#[testkit::tb_serial]
fn coinbase_tip_defaults_to_zero() {
    let dir = util::temp::temp_dir("coinbase_tip");
    let mut bc = Blockchain::new(dir.path().to_str().expect("path"));
    bc.add_account("miner".into(), 0, 0).expect("add miner");

    let block = bc.mine_block("miner").expect("mine block");
    assert_eq!(block.transactions[0].tip, 0);
}

#[testkit::tb_serial]
fn coinbase_claims_proof_rebates() {
    let dir = util::temp::temp_dir("coinbase_rebates");
    let mut bc = Blockchain::new(dir.path().to_str().expect("path"));
    bc.add_account("miner".into(), 0, 0).expect("add miner");

    let relayer_id = b"relay";
    bc.record_proof_relay(relayer_id, 3);
    let before = bc.proof_tracker.snapshot();
    assert_eq!(before.pending_total, 3);
    let (_, info_before) = before
        .relayers
        .iter()
        .find(|(id, _)| id.as_slice() == relayer_id)
        .expect("relayer tracked");
    assert_eq!(info_before.pending, 3);
    assert_eq!(info_before.total_proofs, 3);
    assert_eq!(info_before.total_claimed, 0);
    assert_eq!(info_before.last_claim_height, None);

    let block = bc.mine_block("miner").expect("mine block");
    assert_eq!(block.proof_rebate_ct, TokenAmount::new(3));

    let after = bc.proof_tracker.snapshot();
    assert_eq!(after.pending_total, 0);
    let (_, info_after) = after
        .relayers
        .iter()
        .find(|(id, _)| id.as_slice() == relayer_id)
        .expect("relayer retained");
    assert_eq!(info_after.pending, 0);
    assert_eq!(info_after.total_proofs, 3);
    assert_eq!(info_after.total_claimed, 3);
    assert_eq!(info_after.last_claim_height, Some(block.index));
}
