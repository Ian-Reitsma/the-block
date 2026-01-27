use crypto_suite::hashing::blake3;
use foundation_serialization::json;
use foundation_telemetry::{GovernanceWrapperEntry, WrapperMetricEntry, WrapperSummaryEntry};
use governance::codec::{balance_history_to_json, disbursements_to_json_array};
use governance::treasury::{DisbursementDetails, DisbursementPayload};
use metrics_aggregator::{metrics_registry_guard, router, AppState};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use sys::tempfile;
use the_block::governance::treasury::{mark_cancelled, mark_executed, TreasuryBalanceEventKind};
use the_block::governance::{GovStore, TreasuryBalanceSnapshot, TreasuryDisbursement};

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[test]
fn treasury_metrics_exposed_via_prometheus() {
    let _guard = metrics_registry_guard();
    let dir = tempfile::tempdir().expect("temp dir");
    let treasury_file = dir.path().join("treasury_disbursements.json");

    let scheduled = TreasuryDisbursement::new(1, "tb1dest-1".into(), 100, "memo".into(), 75);
    let mut executed = TreasuryDisbursement::new(2, "tb1dest-2".into(), 200, String::new(), 50);
    mark_executed(&mut executed, "0xfeed".into());
    let mut cancelled = TreasuryDisbursement::new(3, "tb1dest-3".into(), 150, String::new(), 60);
    mark_cancelled(&mut cancelled, "duplicate".into());

    let records = vec![scheduled, executed, cancelled];
    let payload = json::to_vec_value(&disbursements_to_json_array(&records));
    fs::write(&treasury_file, payload).expect("write treasury file");

    let balance_file = dir.path().join("treasury_balance.json");
    let snapshots = vec![TreasuryBalanceSnapshot {
        id: 1,
        balance: 450,
        delta: 450,
        recorded_at: 1,
        event: TreasuryBalanceEventKind::Accrual,
        disbursement_id: None,
    }];
    let balance_payload = json::to_vec_value(&balance_history_to_json(&snapshots));
    fs::write(&balance_file, balance_payload).expect("write balance file");

    let metrics_db = dir.path().join("metrics.db");
    let state = AppState::new_with_opts(
        "token".into(),
        None,
        &metrics_db,
        60,
        None,
        None,
        Some(PathBuf::from(&treasury_file)),
    );
    let app = router(state.clone());

    run_async(async {
        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics response");
        assert_eq!(resp.status(), httpd::StatusCode::OK);
        let body = String::from_utf8(resp.body().to_vec()).expect("metrics utf8");
        assert!(body.contains("treasury_disbursement_count{status=\"draft\"} 1"));
        assert!(body.contains("treasury_disbursement_count{status=\"executed\"} 1"));
        assert!(body.contains("treasury_disbursement_count{status=\"rolled_back\"} 1"));
        assert!(body.contains("treasury_disbursement_pipeline_total{status=\"draft\"} 1"));
        assert!(body.contains("treasury_disbursement_pipeline_total{status=\"executed\"} 1"));
        assert!(body.contains("treasury_disbursement_amount{status=\"executed\"} 200"));
        assert!(body.contains("treasury_disbursement_amount{status=\"rolled_back\"} 150"));
        assert!(body.contains("treasury_disbursement_next_epoch 75"));
        assert!(body.contains("treasury_disbursement_execution_lag_seconds{stat=\"avg\"}"));
        assert!(body.contains("treasury_disbursement_execution_lag_seconds{stat=\"max\"}"));
        assert!(body.contains("treasury_balance_current 450"));
        assert!(body.contains("treasury_balance_snapshot_count 1"));
        assert!(body.contains("treasury_balance_last_delta 450"));
        assert!(body.contains("treasury_balance_last_event_age_seconds"));
    });
}

#[test]
fn treasury_metrics_from_store_source() {
    let _guard = metrics_registry_guard();
    let dir = tempfile::tempdir().expect("temp dir");
    let gov_path = dir.path().join("gov.db");
    let store = GovStore::open(&gov_path);
    store.record_treasury_accrual(600).expect("accrual");
    let queued = store
        .queue_disbursement(DisbursementPayload {
            disbursement: DisbursementDetails {
                destination: "tb1dest-4".into(),
                amount: 120,
                memo: "".into(),
                scheduled_epoch: 400,
                expected_receipts: Vec::new(),
            },
            ..Default::default()
        })
        .expect("queue");
    store
        .execute_disbursement(queued.id, "0xbeef", Vec::new())
        .expect("execute");
    store
        .refresh_executor_lease("lease-holder", Duration::from_secs(120))
        .expect("lease");

    env::set_var(
        "AGGREGATOR_TREASURY_DB",
        gov_path.to_string_lossy().to_string(),
    );
    let metrics_db = dir.path().join("metrics.db");
    let state = AppState::new_with_opts("token".into(), None, &metrics_db, 60, None, None, None);
    env::remove_var("AGGREGATOR_TREASURY_DB");

    let app = router(state.clone());
    run_async(async {
        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics response");
        assert_eq!(resp.status(), httpd::StatusCode::OK);
        let body = String::from_utf8(resp.body().to_vec()).expect("metrics utf8");
        assert!(body.contains("treasury_balance_current"));
        assert!(body.contains("treasury_disbursement_count{status=\"executed\"} 1"));
        assert!(body.contains("treasury_executor_lease_released 0"));
    });
}

#[test]
fn treasury_metrics_accept_legacy_string_fields() {
    let _guard = metrics_registry_guard();
    let dir = tempfile::tempdir().expect("temp dir");
    let treasury_file = dir.path().join("treasury_disbursements.json");

    let scheduled = TreasuryDisbursement::new(5, "tb1legacy".into(), 300, String::new(), 10);
    let mut executed =
        TreasuryDisbursement::new(6, "tb1legacy-dest".into(), 150, String::new(), 11);
    mark_executed(&mut executed, "0xdead".into());
    let payload = json::to_vec_value(&disbursements_to_json_array(&[scheduled, executed]));
    fs::write(&treasury_file, payload).expect("write treasury file");

    let balance_file = dir.path().join("treasury_balance.json");
    let legacy_payload = r#"[
        {
            "id": "9",
            "balance": "450",
            "delta": "450",
            "recorded_at": "12345",
            "event": "ACCRUAL"
        }
    ]"#;
    fs::write(&balance_file, legacy_payload).expect("write legacy balance");

    let metrics_db = dir.path().join("metrics.db");
    let state = AppState::new_with_opts(
        "token".into(),
        None,
        &metrics_db,
        60,
        None,
        None,
        Some(PathBuf::from(&treasury_file)),
    );
    let app = router(state.clone());

    run_async(async {
        let resp = app
            .handle(app.request_builder().path("/metrics").build())
            .await
            .expect("metrics response");
        assert_eq!(resp.status(), httpd::StatusCode::OK);
        let body = String::from_utf8(resp.body().to_vec()).expect("metrics utf8");
        assert!(body.contains("treasury_balance_current 450"));
    });
}

#[test]
fn wrappers_schema_hash_is_stable() {
    let mut map: BTreeMap<String, WrapperSummaryEntry> = BTreeMap::new();
    let mut storage_success_labels = HashMap::new();
    storage_success_labels.insert("status".into(), "success".into());
    let mut storage_error_labels = HashMap::new();
    storage_error_labels.insert("status".into(), "error".into());
    let mut relay_reason_payload = HashMap::new();
    relay_reason_payload.insert("reason".into(), "payload_too_large".into());
    let mut relay_reason_ack = HashMap::new();
    relay_reason_ack.insert("reason".into(), "ack_stale".into());
    let mut relay_reason_budget = HashMap::new();
    relay_reason_budget.insert("reason".into(), "budget_exhausted".into());
    map.insert(
        "node-a".into(),
        WrapperSummaryEntry {
            metrics: vec![
                WrapperMetricEntry {
                    metric: "governance.treasury.executor.last_submitted_nonce".into(),
                    labels: HashMap::new(),
                    value: 7.0,
                },
                WrapperMetricEntry {
                    metric: "storage_discovery_requests_total".into(),
                    labels: HashMap::new(),
                    value: 12.0,
                },
                WrapperMetricEntry {
                    metric: "storage_discovery_results_total".into(),
                    labels: storage_success_labels.clone(),
                    value: 9.0,
                },
                WrapperMetricEntry {
                    metric: "storage_discovery_results_total".into(),
                    labels: storage_error_labels.clone(),
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "relay_receipts_total".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "relay_receipt_bytes_total".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "relay_job_rejected_total".into(),
                    labels: relay_reason_payload.clone(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "relay_job_rejected_total".into(),
                    labels: relay_reason_ack.clone(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "relay_job_rejected_total".into(),
                    labels: relay_reason_budget.clone(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "storage_adoption_plan_coverage_percent".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "storage_adoption_plan_cost_per_share".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "storage_adoption_plan_estimated_total_cost".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "storage_adoption_plan_selected_provider_count".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "storage_adoption_plan_required_provider_count".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
            ],
            governance: Some(GovernanceWrapperEntry {
                treasury_balance: 1_200,
                disbursements_total: 3,
                executed_total: 1,
                rolled_back_total: 1,
                draft_total: 1,
                voting_total: 0,
                queued_total: 0,
                timelocked_total: 0,
                executor_pending_matured: 0,
                executor_staged_intents: 0,
                executor_lease_released: false,
                executor_last_success_at: Some(123),
                executor_last_error_at: None,
            }),
        },
    );
    let value = foundation_serialization::json::to_value(&map).expect("serialize wrappers map");
    let encoded = foundation_serialization::json::to_vec_value(&value);
    if std::env::var("PRINT_WRAPPERS_SNAPSHOT").as_deref() == Ok("1") {
        let serialized =
            String::from_utf8(encoded.clone()).expect("wrappers map utf8 serialization");
        eprintln!("{serialized}");
    }
    if std::env::var("WRITE_WRAPPERS_SNAPSHOT").as_deref() == Ok("1") {
        fs::create_dir_all("tests/snapshots").expect("create snapshots directory");
        fs::write("tests/snapshots/wrappers.json", &encoded)
            .expect("write aggregator wrappers snapshot");
    }
    let hash = blake3::hash(&encoded);
    let hash_hex = hash.to_hex().to_string();
    assert_eq!(
        hash_hex.as_str(),
        "e642a8db353a7f06746aad480a73b460bcb959c3583d6a457e4a1f1503ae7951",
        "wrappers schema or field set drifted; update consumers or refresh the expected hash intentionally"
    );
}
