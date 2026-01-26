#![cfg(feature = "integration-tests")]

use crypto_suite::hex;
use foundation_serialization::json::Value;
use std::env;
use sys::tempfile::tempdir;

use storage_market::slashing::SlashingReason;
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    rpc::storage,
    storage::{
        pipeline::{Provider, StoragePipeline},
        placement::NodeCatalog,
        slash,
    },
    telemetry::consensus_metrics::BLOCK_HEIGHT,
};

struct DummyProvider {
    id: String,
}

impl Provider for DummyProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

#[test]
fn audit_reports_missing_chunks_trigger_slash() {
    Settlement::init("", SettleMode::Real);

    let dir = tempdir().unwrap();
    env::set_var("TB_STORAGE_PIPELINE_DIR", dir.path());

    let mut pipeline = StoragePipeline::open(dir.path().to_str().unwrap());
    pipeline.set_rent_rate(0);

    let mut catalog = NodeCatalog::new();
    catalog.register(DummyProvider {
        id: "prov-1".into(),
    });

    let (receipt, _blob_tx) = pipeline
        .put_object(b"missing-chunk", "lane", &mut catalog)
        .expect("store blob");

    let manifest = pipeline
        .get_manifest(&receipt.manifest_hash)
        .expect("manifest stored");
    let chunk_hash = manifest.chunks.first().expect("chunk").id;
    let chunk_key = format!("chunk/{}", hex::encode(&chunk_hash));
    pipeline.db_mut().remove(&chunk_key);

    let manifest_hex = hex::encode(&receipt.manifest_hash);
    BLOCK_HEIGHT.get().set(100);
    let _ = slash::drain_slash_events(u64::MAX);

    let response = storage::audit(Some(manifest_hex.clone()), None);
    let reports = response
        .get("reports")
        .and_then(|v| v.as_array())
        .expect("reports array");
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].get("status").and_then(Value::as_str),
        Some("reported")
    );
    assert_eq!(
        reports[0].get("provider").and_then(Value::as_str),
        Some("prov-1")
    );

    let slashes = slash::drain_slash_events(110);
    let missing_slash = slashes
        .into_iter()
        .find(|slash_entry| {
            matches!(
            &slash_entry.reason,
            SlashingReason::MissingRepair {
                contract_id,
                chunk_hash: hash
            } if contract_id == &manifest_hex && *hash == chunk_hash
                )
        })
        .expect("missing chunk slash emitted");
    assert_eq!(missing_slash.provider, "prov-1");
}
