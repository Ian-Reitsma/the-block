#![cfg(feature = "integration-tests")]
use sys::tempfile::tempdir;

use the_block::governance::{
    controller, GovStore, ProposalStatus, ReleaseAttestation, ReleaseBallot, ReleaseVote,
    VoteChoice,
};
use the_block::provenance;

use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::OsRng;

#[test]
fn release_flow_approves_hash() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
    let proposal = ReleaseVote::new(hash.clone(), vec![], 0, "tester".into(), 0, 0);
    let id = controller::submit_release(&store, proposal).unwrap();
    let ballot = ReleaseBallot {
        proposal_id: id,
        voter: "tester".into(),
        choice: VoteChoice::Yes,
        weight: 1,
        received_at: 0,
    };
    controller::vote_release(&store, ballot).unwrap();
    let status = controller::tally_release(&store, id, 0).unwrap();
    assert_eq!(status, ProposalStatus::Activated);
    assert!(store.is_release_hash_approved(&hash).unwrap());
}

#[test]
fn release_flow_requires_signature_when_signers_configured() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut rng = OsRng::default();
    let sk1 = SigningKey::generate(&mut rng);
    let sk2 = SigningKey::generate(&mut rng);
    let signer_hex = format!(
        "{},{}",
        crypto_suite::hex::encode(sk1.verifying_key().to_bytes()),
        crypto_suite::hex::encode(sk2.verifying_key().to_bytes())
    );
    std::env::set_var("TB_RELEASE_SIGNERS", &signer_hex);
    provenance::refresh_release_signers();

    let hash = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string();
    let msg = format!("release:{hash}");
    let sig1 = crypto_suite::hex::encode(sk1.sign(msg.as_bytes()).to_bytes());
    let sig2 = crypto_suite::hex::encode(sk2.sign(msg.as_bytes()).to_bytes());
    let attestations = vec![
        ReleaseAttestation {
            signer: crypto_suite::hex::encode(sk1.verifying_key().to_bytes()),
            signature: sig1,
        },
        ReleaseAttestation {
            signer: crypto_suite::hex::encode(sk2.verifying_key().to_bytes()),
            signature: sig2,
        },
    ];
    let proposal = ReleaseVote::new(hash.clone(), attestations, 2, "tester".into(), 0, 0);
    let id = controller::submit_release(&store, proposal).unwrap();
    controller::tally_release(&store, id, 0).unwrap();
    assert!(store.is_release_hash_approved(&hash).unwrap());

    std::env::remove_var("TB_RELEASE_SIGNERS");
    provenance::refresh_release_signers();
}

#[test]
fn release_flow_rejects_missing_signature() {
    let dir = tempdir().unwrap();
    let store = GovStore::open(dir.path());
    let mut rng = OsRng::default();
    let sk1 = SigningKey::generate(&mut rng);
    let sk2 = SigningKey::generate(&mut rng);
    let signer_hex = format!(
        "{},{}",
        crypto_suite::hex::encode(sk1.verifying_key().to_bytes()),
        crypto_suite::hex::encode(sk2.verifying_key().to_bytes())
    );
    std::env::set_var("TB_RELEASE_SIGNERS", &signer_hex);
    provenance::refresh_release_signers();

    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcd".to_string();
    let msg = format!("release:{hash}");
    let sig = crypto_suite::hex::encode(sk1.sign(msg.as_bytes()).to_bytes());
    let attestation = ReleaseAttestation {
        signer: crypto_suite::hex::encode(sk1.verifying_key().to_bytes()),
        signature: sig,
    };
    let proposal = ReleaseVote::new(hash, vec![attestation], 2, "tester".into(), 0, 0);
    assert!(controller::submit_release(&store, proposal).is_err());

    std::env::remove_var("TB_RELEASE_SIGNERS");
    provenance::refresh_release_signers();
}
