use blake3::Hasher;
use the_block::storage::erasure;
use the_block::storage::repair::{self, RepairLog, RepairLogStatus, RepairRequest};
use the_block::storage::types::{ChunkRef, ObjectManifest, ProviderChunkEntry, Redundancy};
use the_block::SimpleDb;

use tempfile::tempdir;
use the_block::storage::types::{CHACHA20_POLY1305_NONCE_LEN, CHACHA20_POLY1305_TAG_LEN};

fn store_manifest(db: &mut SimpleDb, manifest: &mut ObjectManifest) -> [u8; 32] {
    let mut tmp = manifest.clone();
    tmp.blake3 = [0u8; 32];
    let bytes = bincode::serialize(&tmp).expect("serialize");
    let mut hasher = Hasher::new();
    hasher.update(&bytes);
    let hash = hasher.finalize();
    let mut manifest_hash = [0u8; 32];
    manifest_hash.copy_from_slice(hash.as_bytes());
    manifest.blake3 = manifest_hash;
    let manifest_bytes = bincode::serialize(manifest).expect("serialize final");
    db.try_insert(
        &format!("manifest/{}", hex::encode(manifest_hash)),
        manifest_bytes,
    )
    .expect("store manifest");
    manifest_hash
}

fn write_shards(db: &mut SimpleDb, manifest: &ObjectManifest, shards: &[Vec<u8>]) {
    for (idx, chunk_ref) in manifest.chunks.iter().enumerate() {
        let key = format!("chunk/{}", hex::encode(chunk_ref.id));
        db.try_insert(&key, shards[idx].clone())
            .expect("store shard");
    }
}

fn sample_manifest(chunk_bytes: usize) -> (ObjectManifest, Vec<Vec<u8>>) {
    let (rs_data, rs_parity) = erasure::reed_solomon_counts();
    let total_shards = erasure::total_shards_per_chunk();
    let chunk_plain = chunk_bytes;
    let cipher_len = chunk_plain + CHACHA20_POLY1305_NONCE_LEN + CHACHA20_POLY1305_TAG_LEN;
    let chunk = vec![0xAB; cipher_len];
    let shards = erasure::encode(&chunk).expect("encode");
    assert_eq!(shards.len(), total_shards);

    let mut chunks = Vec::with_capacity(total_shards);
    for (idx, shard) in shards.iter().enumerate() {
        let mut h = Hasher::new();
        h.update(&[idx as u8]);
        h.update(shard);
        let mut id = [0u8; 32];
        id.copy_from_slice(h.finalize().as_bytes());
        chunks.push(ChunkRef {
            id,
            nodes: Vec::new(),
            provider_chunks: Vec::new(),
        });
    }

    let manifest = ObjectManifest {
        version: 1,
        total_len: chunk_plain as u64,
        chunk_len: chunk_plain as u32,
        chunks,
        redundancy: Redundancy::ReedSolomon {
            data: rs_data as u8,
            parity: rs_parity as u8,
        },
        content_key_enc: vec![0u8; 32],
        blake3: [0u8; 32],
        chunk_lens: vec![chunk_plain as u32],
        provider_chunks: Vec::<ProviderChunkEntry>::new(),
    };
    (manifest, shards)
}

#[test]
fn repairs_missing_shards_and_logs_success() {
    let dir = tempdir().expect("dir");
    let path = dir.path().join("db");
    let mut db = SimpleDb::open(path.to_str().unwrap());
    let (mut manifest, shards) = sample_manifest(2048);
    let manifest_hash = store_manifest(&mut db, &mut manifest);
    write_shards(&mut db, &manifest, &shards);

    let log = RepairLog::new(dir.path().join("repair_log"));
    // Remove a few shards to force reconstruction.
    for idx in [0usize, 3, 7] {
        let key = format!("chunk/{}", hex::encode(manifest.chunks[idx].id));
        db.remove(&key);
    }

    let summary = repair::run_once(&mut db, &log, RepairRequest::default()).expect("run");
    assert_eq!(summary.successes, 1);
    assert!(summary.failures == 0);
    assert!(summary.bytes_repaired > 0);

    // Ensure shards were rewritten.
    for idx in [0usize, 3, 7] {
        let key = format!("chunk/{}", hex::encode(manifest.chunks[idx].id));
        assert!(db.get(&key).is_some());
    }

    let entries = log.recent_entries(10).expect("read log");
    assert!(entries
        .iter()
        .any(|entry| entry.status == RepairLogStatus::Success));

    #[cfg(feature = "telemetry")]
    {
        let attempts = the_block::telemetry::STORAGE_REPAIR_ATTEMPTS_TOTAL
            .with_label_values(&["success"])
            .get();
        assert!(attempts >= 1);
    }

    // Ensure manifest hash key exists
    let manifest_key = format!("manifest/{}", hex::encode(manifest_hash));
    assert!(db.get(&manifest_key).is_some());
}

#[test]
fn detects_corrupt_manifest_and_logs_failure() {
    let dir = tempdir().expect("dir");
    let path = dir.path().join("db");
    let mut db = SimpleDb::open(path.to_str().unwrap());
    let (mut manifest, shards) = sample_manifest(1024);
    let manifest_hash = store_manifest(&mut db, &mut manifest);
    write_shards(&mut db, &manifest, &shards);

    // Corrupt the stored manifest hash field.
    let manifest_key = format!("manifest/{}", hex::encode(manifest_hash));
    let mut stored = db.get(&manifest_key).expect("manifest stored");
    stored[10] ^= 0xFF;
    db.insert(&manifest_key, stored);

    let log = RepairLog::new(dir.path().join("repair_log"));
    let summary = repair::run_once(&mut db, &log, RepairRequest::default()).expect("run");
    assert_eq!(summary.successes, 0);
    assert!(summary.failures >= 1);

    let entries = log.recent_entries(10).expect("entries");
    assert!(entries
        .iter()
        .any(|entry| entry.status == RepairLogStatus::Failure
            && entry.error.as_deref() == Some("manifest hash mismatch")));
}

#[test]
fn applies_backoff_after_repeated_failures() {
    let dir = tempdir().expect("dir");
    let path = dir.path().join("db");
    let mut db = SimpleDb::open(path.to_str().unwrap());
    let (mut manifest, shards) = sample_manifest(1024);
    store_manifest(&mut db, &mut manifest);
    write_shards(&mut db, &manifest, &shards);

    let log = RepairLog::new(dir.path().join("repair_log"));

    // Remove enough shards to force reconstruction failure (all data shards).
    let (rs_data, _) = erasure::reed_solomon_counts();
    for idx in 0..rs_data {
        let key = format!("chunk/{}", hex::encode(manifest.chunks[idx].id));
        db.remove(&key);
    }

    let summary1 = repair::run_once(&mut db, &log, RepairRequest::default()).expect("run1");
    assert_eq!(summary1.failures, 1);
    let summary2 = repair::run_once(&mut db, &log, RepairRequest::default()).expect("run2");
    assert_eq!(summary2.skipped, 1);

    let entries = log.recent_entries(10).expect("entries");
    assert!(entries
        .iter()
        .any(|entry| entry.status == RepairLogStatus::Skipped));

    #[cfg(feature = "telemetry")]
    {
        let failures = the_block::telemetry::STORAGE_REPAIR_FAILURES_TOTAL
            .with_label_values(&["reconstruct"])
            .get();
        assert!(failures >= 1);
    }
}
