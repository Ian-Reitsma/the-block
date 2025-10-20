use foundation_serialization::binary_cursor::Writer;
use the_block::{
    identity::{
        did::{DidAttestationRecord, DidRecord, DidRegistry},
        did_binary, handle_binary,
        handle_registry::{HandleRecord, HandleRegistry},
    },
    simple_db::{names, SimpleDb},
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
        let legacy_bytes = encode_legacy_did(&record_legacy);
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

        let legacy_bytes = encode_legacy_handle(&record_legacy);
        db.insert(&format!("handles/{}", "legacy"), legacy_bytes);
        let owner_bytes = handle_binary::encode_string("legacy");
        db.insert(&format!("owners/{}", record_legacy.address), owner_bytes);
        let nonce_bytes = handle_binary::encode_u64(record_legacy.nonce);
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

fn encode_legacy_did(record: &DidRecord) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|s| {
        s.field_string("address", &record.address);
        s.field_string("document", &record.document);
        s.field_with("hash", |w| w.write_bytes(&record.hash));
        s.field_u64("nonce", record.nonce);
        s.field_u64("updated_at", record.updated_at);
        s.field_with("public_key", |w| w.write_bytes(&record.public_key));
        s.field_with("remote_attestation", |w| match &record.remote_attestation {
            Some(attestation) => {
                w.write_bool(true);
                w.write_struct(|att| {
                    att.field_string("signer", &attestation.signer);
                    att.field_string("signature", &attestation.signature);
                });
            }
            None => w.write_bool(false),
        });
    });
    writer.finish()
}

fn encode_legacy_handle(record: &HandleRecord) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.write_struct(|s| {
        s.field_string("address", &record.address);
        s.field_u64("created_at", record.created_at);
        s.field_with("attest_sig", |w| w.write_bytes(&record.attest_sig));
        s.field_u64("nonce", record.nonce);
        s.field_with("version", |w| w.write_u16(record.version));
        #[cfg(feature = "pq-crypto")]
        {
            s.field_with("pq_pubkey", |w| {
                w.write_option_with(record.pq_pubkey.as_ref(), |writer, value| {
                    writer.write_bytes(value);
                });
            });
        }
    });
    writer.finish()
}
