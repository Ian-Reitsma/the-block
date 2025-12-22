#![forbid(unsafe_code)]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crypto_suite::hex;
use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use storage::{merkle_proof::MerkleTree, StorageContract};
use storage_market::{ContractRecord, ReplicaIncentive};
use storage_market::{
    ImportMode, ManifestSource, ManifestStatus, StorageImporter, StorageMarket,
    LEGACY_MANIFEST_FILE, MIGRATED_MANIFEST_PREFIX,
};
use sys::tempfile::tempdir;

type TestResult<T> = Result<T, Box<dyn Error>>;

fn demo_chunks_for_import() -> Vec<Vec<u8>> {
    vec![
        b"import-chunk-0".to_vec(),
        b"import-chunk-1".to_vec(),
        b"import-chunk-2".to_vec(),
    ]
}

fn write_manifest(dir: &Path, records: &[ContractRecord]) -> TestResult<PathBuf> {
    const LEGACY_TREE_HEX: &str = "6d61726b65742f636f6e747261637473";
    let mut trees = JsonMap::new();
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let key_bytes = record.contract.object_id.as_bytes();
        let value = contract_record_to_value(record);
        let encoded = json::to_vec_value(&value);
        let mut entry = JsonMap::new();
        entry.insert("key".into(), JsonValue::String(hex::encode(key_bytes)));
        entry.insert("value".into(), JsonValue::String(hex::encode(&encoded)));
        entries.push(JsonValue::Object(entry));
    }
    trees.insert(LEGACY_TREE_HEX.into(), JsonValue::Array(entries));
    let mut root = JsonMap::new();
    root.insert("trees".into(), JsonValue::Object(trees));
    let bytes = json::to_vec_value(&JsonValue::Object(root));
    fs::create_dir_all(dir)?;
    let path = dir.join(LEGACY_MANIFEST_FILE);
    fs::write(&path, bytes)?;
    Ok(path)
}

fn contract_record(object_id: &str, provider: &str) -> ContractRecord {
    let chunks = demo_chunks_for_import();
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    let tree = MerkleTree::build(&chunk_refs).expect("merkle tree");
    let contract = StorageContract {
        object_id: object_id.into(),
        provider_id: provider.into(),
        original_bytes: 4096,
        shares: 4,
        price_per_block: 3,
        start_block: 0,
        retention_blocks: 12,
        next_payment_block: 1,
        accrued: 0,
        total_deposit_ct: 0,
        last_payment_block: None,
        storage_root: tree.root,
    };
    let replicas = vec![ReplicaIncentive::new(provider.into(), 4, 3, 15)];
    ContractRecord::with_replicas(contract, replicas)
}

fn contract_record_to_value(record: &ContractRecord) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "contract".into(),
        storage_contract_to_value(&record.contract),
    );
    let replicas = record
        .replicas
        .iter()
        .map(replica_to_value)
        .collect::<Vec<_>>();
    map.insert("replicas".into(), JsonValue::Array(replicas));
    JsonValue::Object(map)
}

fn storage_contract_to_value(contract: &StorageContract) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "object_id".into(),
        JsonValue::String(contract.object_id.clone()),
    );
    map.insert(
        "provider_id".into(),
        JsonValue::String(contract.provider_id.clone()),
    );
    map.insert(
        "original_bytes".into(),
        JsonValue::Number(JsonNumber::from(contract.original_bytes)),
    );
    map.insert(
        "shares".into(),
        JsonValue::Number(JsonNumber::from(contract.shares)),
    );
    map.insert(
        "price_per_block".into(),
        JsonValue::Number(JsonNumber::from(contract.price_per_block)),
    );
    map.insert(
        "start_block".into(),
        JsonValue::Number(JsonNumber::from(contract.start_block)),
    );
    map.insert(
        "retention_blocks".into(),
        JsonValue::Number(JsonNumber::from(contract.retention_blocks)),
    );
    map.insert(
        "next_payment_block".into(),
        JsonValue::Number(JsonNumber::from(contract.next_payment_block)),
    );
    map.insert(
        "accrued".into(),
        JsonValue::Number(JsonNumber::from(contract.accrued)),
    );
    map.insert(
        "total_deposit_ct".into(),
        JsonValue::Number(JsonNumber::from(contract.total_deposit_ct)),
    );
    map.insert(
        "last_payment_block".into(),
        contract
            .last_payment_block
            .map(|block| JsonValue::Number(JsonNumber::from(block)))
            .unwrap_or(JsonValue::Null),
    );
    map.insert(
        "storage_root".into(),
        JsonValue::Array(
            contract
                .storage_root
                .as_bytes()
                .iter()
                .map(|byte| JsonValue::Number(JsonNumber::from(*byte)))
                .collect(),
        ),
    );
    JsonValue::Object(map)
}

fn replica_to_value(replica: &ReplicaIncentive) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "provider_id".into(),
        JsonValue::String(replica.provider_id.clone()),
    );
    map.insert(
        "allocated_shares".into(),
        JsonValue::Number(JsonNumber::from(replica.allocated_shares)),
    );
    map.insert(
        "price_per_block".into(),
        JsonValue::Number(JsonNumber::from(replica.price_per_block)),
    );
    map.insert(
        "deposit_ct".into(),
        JsonValue::Number(JsonNumber::from(replica.deposit_ct)),
    );
    map.insert(
        "proof_successes".into(),
        JsonValue::Number(JsonNumber::from(replica.proof_successes)),
    );
    map.insert(
        "proof_failures".into(),
        JsonValue::Number(JsonNumber::from(replica.proof_failures)),
    );
    map.insert(
        "last_proof_block".into(),
        replica
            .last_proof_block
            .map(|block| JsonValue::Number(JsonNumber::from(block)))
            .unwrap_or(JsonValue::Null),
    );
    map.insert(
        "last_outcome".into(),
        replica
            .last_outcome
            .as_ref()
            .map(|outcome| match outcome {
                storage_market::ProofOutcome::Success => "Success",
                storage_market::ProofOutcome::Failure => "Failure",
            })
            .map(|label| JsonValue::String(label.into()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(map)
}

#[test]
fn importer_summarizes_and_replays_manifest() -> TestResult<()> {
    let dir = tempdir()?;
    let base = dir.path().join("market");
    let records = vec![
        contract_record("obj-a", "prov-a"),
        contract_record("obj-b", "prov-b"),
    ];
    write_manifest(&base, &records)?;

    let importer = StorageImporter::open(&base)?;

    let summary = importer.summarize(ManifestSource::Auto)?;
    assert_eq!(summary.total_entries, records.len());
    assert_eq!(summary.present, 0);
    assert_eq!(summary.missing, records.len());
    assert!(matches!(summary.status, ManifestStatus::Pending { .. }));
    assert!(summary.source_path.is_some());

    let stats = importer
        .import(ManifestSource::Pending, ImportMode::InsertMissing)
        .expect("initial pending import");
    assert_eq!(stats.applied, records.len());
    assert_eq!(stats.skipped_existing, 0);
    assert_eq!(stats.overwritten, 0);

    let repeat = importer
        .import(ManifestSource::Pending, ImportMode::InsertMissing)
        .expect("repeat pending import");
    assert_eq!(repeat.applied, 0);
    assert_eq!(repeat.skipped_existing, records.len());
    assert_eq!(repeat.no_change, 0);

    let overwrite = importer
        .import(ManifestSource::Pending, ImportMode::OverwriteExisting)
        .expect("overwrite pending import");
    assert_eq!(overwrite.applied, 0);
    assert_eq!(overwrite.overwritten, 0);
    assert_eq!(overwrite.no_change, records.len());

    let mut modified = records.clone();
    modified[0].contract.price_per_block = modified[0].contract.price_per_block.saturating_add(5);
    modified[0].replicas[0].price_per_block =
        modified[0].replicas[0].price_per_block.saturating_add(5);
    write_manifest(&base, &modified)?;

    let changed = importer
        .import(ManifestSource::Pending, ImportMode::OverwriteExisting)
        .expect("overwrite modified manifest");
    assert_eq!(changed.applied, 0);
    assert_eq!(changed.overwritten, 1);
    assert_eq!(changed.no_change, modified.len() - 1);

    let manifest_digest = importer
        .manifest_checksum(ManifestSource::Pending)?
        .expect("manifest checksum present");
    let db_digest = importer.database_checksum(storage_market::ChecksumScope::ContractsOnly)?;
    assert_eq!(manifest_digest.hash, db_digest.hash);
    assert_eq!(manifest_digest.entries, db_digest.entries);

    let comparison = importer
        .verify(
            ManifestSource::Pending,
            storage_market::ChecksumScope::ContractsOnly,
        )?
        .manifest
        .expect("comparison manifest present");
    assert_eq!(comparison.hash, db_digest.hash);

    let market = StorageMarket::open(&base)?;
    let persisted = market.contracts()?;
    assert_eq!(persisted.len(), records.len());
    assert!(persisted
        .iter()
        .any(|record| record.contract.object_id == "obj-a"));

    let migrated_path = base.join(format!("{MIGRATED_MANIFEST_PREFIX}.json"));
    assert!(migrated_path.exists());
    println!("importer base path: {}", importer.base_path().display());
    println!("migrated path: {}", migrated_path.display());

    let migrated_summary = importer.summarize(ManifestSource::Auto)?;
    assert!(matches!(
        migrated_summary.status,
        ManifestStatus::Migrated { .. }
    ));
    assert_eq!(
        migrated_summary.source_path.as_ref().map(PathBuf::as_path),
        Some(migrated_path.as_path())
    );

    let explicit_migrated_summary = importer.summarize(ManifestSource::Migrated)?;
    assert_eq!(
        explicit_migrated_summary
            .source_path
            .as_ref()
            .map(PathBuf::as_path),
        Some(migrated_path.as_path())
    );

    let migrated_stats = importer
        .import(
            ManifestSource::File(migrated_path.clone()),
            ImportMode::OverwriteExisting,
        )
        .expect("import from migrated manifest");
    assert_eq!(migrated_stats.overwritten, 0);
    assert_eq!(migrated_stats.no_change, records.len());

    Ok(())
}
