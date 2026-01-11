use crypto_suite::hex;
use explorer::{compute_view, Explorer, ProviderSettlementRecord};
use sys::tempfile;
use the_block::compute_market::{
    settlement::{SlaResolution, SlaResolutionKind},
    snark::{self, SnarkBackend},
    workloads,
};

#[test]
fn provider_balances_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");
    let records = vec![ProviderSettlementRecord {
        provider: "alice".to_string(),
        consumer: 42,
        industrial: 7,
        updated_at: 123,
    }];
    explorer
        .index_settlement_balances(&records)
        .expect("index balances");
    let stored = compute_view::provider_balances(&explorer).expect("fetch balances");
    assert_eq!(stored, records);
}

#[test]
fn sla_history_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");
    let wasm = b"proof-workload";
    let output = workloads::snark::run(wasm);
    let proof = snark::prove_with_backend(wasm, &output, SnarkBackend::Cpu).expect("cpu proof");
    let entry = SlaResolution {
        job_id: "job-1".to_string(),
        provider: "alice".to_string(),
        buyer: "buyer".to_string(),
        outcome: SlaResolutionKind::Completed,
        burned: 0,
        refunded: 0,
        deadline: 1,
        resolved_at: 2,
        proofs: vec![proof.clone()],
    };
    explorer
        .record_sla_history(&[entry])
        .expect("record sla history");
    let stored = explorer.compute_sla_history(4).expect("fetch sla history");
    assert_eq!(stored.len(), 1);
    let record = &stored[0];
    assert_eq!(record.job_id, "job-1");
    assert_eq!(record.proofs.len(), 1);
    assert_eq!(
        record.proofs[0].fingerprint,
        hex::encode(proof.fingerprint())
    );
    assert_eq!(record.proofs[0].backend, "CPU");
    let rebuilt = record.proofs[0]
        .to_bundle()
        .expect("rehydrate explorer proof");
    assert!(snark::verify(&rebuilt, wasm, &output).expect("verify restored bundle"));
}
