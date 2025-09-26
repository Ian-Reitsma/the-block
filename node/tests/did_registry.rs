#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::{
    ed25519::{Signature, SigningKey, VerifyingKey, SIGNATURE_LENGTH},
    Signer,
};
use std::convert::TryInto;
use tempfile::tempdir;
use the_block::generate_keypair;
use the_block::governance::GovStore;
use the_block::identity::{DidError, DidRegistry};
use the_block::transaction::{TxDidAnchor, TxDidAnchorAttestation};

fn build_anchor(doc: &str, nonce: u64, sk: &SigningKey) -> TxDidAnchor {
    let pk = sk.verifying_key();
    let pk_bytes = pk.to_bytes();
    let mut tx = TxDidAnchor {
        address: hex::encode(pk_bytes),
        public_key: pk_bytes.to_vec(),
        document: doc.to_string(),
        nonce,
        signature: Vec::new(),
        remote_attestation: None,
    };
    let sig = sk.sign(tx.owner_digest().as_ref());
    tx.signature = sig.to_bytes().to_vec();
    tx
}

#[test]
fn anchor_roundtrip_and_replay_guard() {
    let dir = tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let mut anchor = build_anchor("{\"id\":1}", 1, &sk);

    let rec = registry
        .anchor(&anchor, Some(&gov))
        .expect("first anchor succeeds");
    assert_eq!(rec.nonce, 1);
    assert_eq!(rec.document, "{\"id\":1}");

    let resolved = registry
        .resolve(&anchor.address)
        .expect("resolve returns record");
    assert_eq!(resolved.hash, rec.hash);

    // replay with same nonce rejected
    let err = registry.anchor(&anchor, Some(&gov)).unwrap_err();
    assert_eq!(err, DidError::Replay);

    // higher nonce accepted
    anchor.nonce = 2;
    let sig = sk.sign(anchor.owner_digest().as_ref());
    anchor.signature = sig.to_bytes().to_vec();
    registry
        .anchor(&anchor, Some(&gov))
        .expect("monotonic update");
}

#[test]
fn governance_revocation_blocks_updates() {
    let dir = tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let anchor = build_anchor("{\"id\":2}", 1, &sk);

    gov.revoke_did(&anchor.address, "compromised", 99)
        .expect("record revocation");
    let err = registry.anchor(&anchor, Some(&gov)).unwrap_err();
    assert_eq!(err, DidError::Revoked);
}

#[test]
fn remote_attestation_accepted() {
    let dir = tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (owner_sk_bytes, _) = generate_keypair();
    let owner_sk = SigningKey::from_bytes(&owner_sk_bytes.try_into().unwrap());
    let mut anchor = build_anchor("{\"id\":3}", 1, &owner_sk);

    let (att_sk_bytes, att_pk_bytes) = generate_keypair();
    let att_sk = SigningKey::from_bytes(&att_sk_bytes.try_into().unwrap());
    let att_signer_hex = hex::encode(att_pk_bytes);
    std::env::set_var("TB_RELEASE_SIGNERS", &att_signer_hex);
    the_block::provenance::refresh_release_signers();

    let att_sig = att_sk.sign(anchor.remote_digest().as_ref());
    anchor.remote_attestation = Some(TxDidAnchorAttestation {
        signer: att_signer_hex.clone(),
        signature: hex::encode(att_sig.to_bytes()),
    });

    registry
        .anchor(&anchor, Some(&gov))
        .expect("anchor with attestation");

    std::env::remove_var("TB_RELEASE_SIGNERS");
    the_block::provenance::refresh_release_signers();
}

#[test]
fn anchor_signature_roundtrip_bytes() {
    let dir = tempdir().unwrap();
    let did_path = dir.path().join("did.db");
    let mut registry = DidRegistry::open(&did_path);
    let gov = GovStore::open(dir.path().join("gov.db"));

    let (sk_bytes, _) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes.try_into().unwrap());
    let anchor = build_anchor("{\"id\":4}", 1, &sk);

    let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
    sig_bytes.copy_from_slice(&anchor.signature);
    let signature = Signature::from_bytes(&sig_bytes);

    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(&anchor.public_key);
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).expect("verifying key");
    verifying_key
        .verify(anchor.owner_digest().as_ref(), &signature)
        .expect("verify");

    registry
        .anchor(&anchor, Some(&gov))
        .expect("anchor persists");
}
