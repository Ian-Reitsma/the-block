use the_block::{
    identity::{
        did::{DidAttestationRecord, DidRecord, DidRegistry},
        did_binary, handle_binary,
        handle_registry::{HandleRecord, HandleRegistry},
    },
    simple_db::{names, SimpleDb},
    util::binary_codec,
};

use sys::tempfile::{tempdir, TempDir};

fn setup_db_path(prefix: &str) -> (TempDir, String) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join(prefix);
    std::fs::create_dir_all(&path).expect("create dir");
    (dir, path.to_string_lossy().to_string())
}

#[test]
fn did_registry_handles_mixed_snapshots() {
    let (_dir_guard, path) = setup_db_path("did-registry");

    let record_current = DidRecord {
        address: "did:block:current".into(),
        document: "{\"service\":[{\"type\":\"svc\"}]}".into(),
        hash: [0x11; 32],
        nonce: 7,
        updated_at: 1_700_000_000,
        public_key: vec![1, 2, 3, 4],
        remote_attestation: Some(DidAttestationRecord {
            signer: "feedface".into(),
            signature: "cafebabe".into(),
        }),
    };

    let record_legacy = DidRecord {
        address: "did:block:legacy".into(),
        document: "{\"assertion\":true}".into(),
        hash: [0x22; 32],
        nonce: 99,
        updated_at: 1_600_000_000,
        public_key: vec![9, 9, 9],
        remote_attestation: None,
    };

    {
        let mut db = SimpleDb::open_named(names::IDENTITY_DID, &path);
        let current_bytes = did_binary::encode_record(&record_current);
        db.insert(&format!("did/{}", record_current.address), current_bytes);
        let legacy_bytes = binary_codec::serialize(&record_legacy).expect("legacy encode");
        db.insert(&format!("did/{}", record_legacy.address), legacy_bytes);
    }

    let registry = DidRegistry::open(&path);
    let loaded_current = registry
        .resolve(&record_current.address)
        .expect("decode current");
    assert_eq!(loaded_current, record_current);

    let loaded_legacy = registry
        .resolve(&record_legacy.address)
        .expect("decode legacy");
    assert_eq!(loaded_legacy, record_legacy);
}

#[test]
fn handle_registry_reads_mixed_snapshots() {
    let (_dir_guard, path) = setup_db_path("handle-registry");

    let record_current = HandleRecord {
        address: "deadbeef".into(),
        created_at: 1_700_000_100,
        attest_sig: vec![1, 3, 3, 7],
        nonce: 4,
        version: 1,
        #[cfg(feature = "pq-crypto")]
        pq_pubkey: None,
    };

    let record_legacy = HandleRecord {
        address: "cafebabe".into(),
        created_at: 1_650_000_000,
        attest_sig: vec![9, 9, 9],
        nonce: 10,
        version: 2,
        #[cfg(feature = "pq-crypto")]
        pq_pubkey: None,
    };

    {
        let mut db = SimpleDb::open_named(names::IDENTITY_HANDLES, &path);
        db.insert(
            &format!("handles/{}", "current"),
            handle_binary::encode_record(&record_current),
        );
        db.insert(
            &format!("owners/{}", record_current.address),
            handle_binary::encode_string("current"),
        );
        db.insert(
            &format!("nonces/{}", record_current.address),
            handle_binary::encode_u64(record_current.nonce),
        );

        let legacy_bytes = binary_codec::serialize(&record_legacy).expect("legacy encode");
        db.insert(&format!("handles/{}", "legacy"), legacy_bytes);
        let owner_bytes = binary_codec::serialize(&"legacy".to_string()).expect("legacy owner");
        db.insert(&format!("owners/{}", record_legacy.address), owner_bytes);
        let nonce_bytes = binary_codec::serialize(&record_legacy.nonce).expect("legacy nonce");
        db.insert(&format!("nonces/{}", record_legacy.address), nonce_bytes);
    }

    let registry = HandleRegistry::open(&path);

    let address = registry.resolve_handle("current").expect("current handle");
    assert_eq!(address, record_current.address);
    let handle = registry
        .handle_of(&record_current.address)
        .expect("current owner");
    assert_eq!(handle, "current");

    let legacy_address = registry.resolve_handle("legacy").expect("legacy handle");
    assert_eq!(legacy_address, record_legacy.address);
    let legacy_handle = registry
        .handle_of(&record_legacy.address)
        .expect("legacy owner");
    assert_eq!(legacy_handle, "legacy");
}
