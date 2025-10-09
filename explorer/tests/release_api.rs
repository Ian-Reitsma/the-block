use crypto_suite::signatures::ed25519::SigningKey;
use explorer::release_view::{paginated_release_history, ReleaseHistoryFilter};
use hex;
use rand::rngs::OsRng;
use sys::tempfile;
use the_block::governance::{
    self, controller, GovStore, ProposalStatus, ReleaseAttestation, ReleaseVote, VoteChoice,
};
use the_block::provenance;

#[test]
fn release_api_paginates_and_filters() {
    let dir = tempfile::tempdir().unwrap();
    let store = GovStore::open(dir.path());

    let mut rng = OsRng::default();
    let signer_a = SigningKey::generate(&mut rng);
    let signer_b = SigningKey::generate(&mut rng);
    let signer_env = format!(
        "{},{}",
        hex::encode(signer_a.verifying_key().to_bytes()),
        hex::encode(signer_b.verifying_key().to_bytes())
    );
    std::env::set_var("TB_RELEASE_SIGNERS", &signer_env);
    provenance::refresh_release_signers();

    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
    let msg = format!("release:{hash}");
    let att_a = ReleaseAttestation {
        signer: hex::encode(signer_a.verifying_key().to_bytes()),
        signature: hex::encode(signer_a.sign(msg.as_bytes()).to_bytes()),
    };
    let att_b = ReleaseAttestation {
        signer: hex::encode(signer_b.verifying_key().to_bytes()),
        signature: hex::encode(signer_b.sign(msg.as_bytes()).to_bytes()),
    };
    let proposal = ReleaseVote::new(hash.clone(), vec![att_a, att_b], 2, "tester".into(), 0, 0);
    let id = controller::submit_release(&store, proposal).unwrap();
    let ballot = governance::ReleaseBallot {
        proposal_id: id,
        voter: "tester".into(),
        choice: VoteChoice::Yes,
        weight: 1,
        received_at: 0,
    };
    controller::vote_release(&store, ballot).unwrap();
    let status = controller::tally_release(&store, id, 0).unwrap();
    assert_eq!(status, ProposalStatus::Activated);
    controller::record_release_install(&store, &hash).unwrap();
    controller::record_release_install(&store, &hash).unwrap();

    let page =
        paginated_release_history(dir.path(), 0, 10, ReleaseHistoryFilter::default()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.entries.len(), 1);
    let entry = &page.entries[0];
    assert_eq!(entry.build_hash, hash);
    assert_eq!(entry.install_count, 2);
    assert!(entry.quorum_met);

    let filtered = paginated_release_history(
        dir.path(),
        0,
        10,
        ReleaseHistoryFilter {
            proposer: Some("nobody".into()),
            start_epoch: None,
            end_epoch: None,
        },
    )
    .unwrap();
    assert_eq!(filtered.total, 0);

    std::env::remove_var("TB_RELEASE_SIGNERS");
    provenance::refresh_release_signers();
}
