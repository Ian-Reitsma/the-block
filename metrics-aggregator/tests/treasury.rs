use foundation_serialization::json;
use governance::codec::{balance_history_to_json, disbursements_to_json_array};
use metrics_aggregator::{router, AppState};
use std::env;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use sys::tempfile;
use the_block::governance::treasury::{mark_cancelled, mark_executed, TreasuryBalanceEventKind};
use the_block::governance::{GovStore, TreasuryBalanceSnapshot, TreasuryDisbursement};

fn run_async<T>(future: impl Future<Output = T>) -> T {
    runtime::block_on(future)
}

#[test]
fn treasury_metrics_exposed_via_prometheus() {
    let dir = tempfile::tempdir().expect("temp dir");
    let treasury_file = dir.path().join("treasury_disbursements.json");

    let scheduled = TreasuryDisbursement::new(1, "dest-1".into(), 100, "memo".into(), 75);
    let mut executed = TreasuryDisbursement::new(2, "dest-2".into(), 200, String::new(), 50);
    mark_executed(&mut executed, "0xfeed".into());
    let mut cancelled = TreasuryDisbursement::new(3, "dest-3".into(), 150, String::new(), 60);
    mark_cancelled(&mut cancelled, "duplicate".into());

    let records = vec![scheduled, executed, cancelled];
    let payload = json::to_vec_value(&disbursements_to_json_array(&records));
    fs::write(&treasury_file, payload).expect("write treasury file");

    let balance_file = dir.path().join("treasury_balance.json");
    let snapshots = vec![TreasuryBalanceSnapshot {
        id: 1,
        balance_ct: 450,
        delta_ct: 450,
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
        assert!(body.contains("treasury_disbursement_count{status=\"scheduled\"} 1"));
        assert!(body.contains("treasury_disbursement_count{status=\"executed\"} 1"));
        assert!(body.contains("treasury_disbursement_count{status=\"cancelled\"} 1"));
        assert!(body.contains("treasury_disbursement_amount_ct{status=\"executed\"} 200"));
        assert!(body.contains("treasury_disbursement_amount_ct{status=\"cancelled\"} 150"));
        assert!(body.contains("treasury_disbursement_next_epoch 75"));
        assert!(body.contains("treasury_balance_current_ct 450"));
        assert!(body.contains("treasury_balance_snapshot_count 1"));
        assert!(body.contains("treasury_balance_last_delta_ct 450"));
        assert!(body.contains("treasury_balance_last_event_age_seconds"));
    });
}

#[test]
fn treasury_metrics_from_store_source() {
    let dir = tempfile::tempdir().expect("temp dir");
    let gov_path = dir.path().join("gov.db");
    let store = GovStore::open(&gov_path);
    store.record_treasury_accrual(600).expect("accrual");
    let queued = store
        .queue_disbursement("dest-4", 120, "", 400)
        .expect("queue");
    store
        .execute_disbursement(queued.id, "0xbeef")
        .expect("execute");

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
        assert!(body.contains("treasury_balance_current_ct"));
        assert!(body.contains("treasury_disbursement_count{status=\"executed\"} 1"));
    });
}

#[test]
fn treasury_metrics_accept_legacy_string_fields() {
    let dir = tempfile::tempdir().expect("temp dir");
    let treasury_file = dir.path().join("treasury_disbursements.json");

    let scheduled = TreasuryDisbursement::new(5, "legacy".into(), 300, String::new(), 10);
    let mut executed = TreasuryDisbursement::new(6, "legacy-dest".into(), 150, String::new(), 11);
    mark_executed(&mut executed, "0xdead".into());
    let payload = json::to_vec_value(&disbursements_to_json_array(&[scheduled, executed]));
    fs::write(&treasury_file, payload).expect("write treasury file");

    let balance_file = dir.path().join("treasury_balance.json");
    let legacy_payload = r#"[
        {
            "id": "9",
            "balance_ct": "450",
            "delta_ct": "450",
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
        assert!(body.contains("treasury_balance_current_ct 450"));
    });
}
