use explorer::{
    build_executor_report, router, Explorer, ExplorerHttpState, TreasuryDisbursementFilter,
    TreasuryDisbursementStatusFilter, TreasuryTimelineEntry, TreasuryTimelineFilter,
};
use foundation_serialization::json;
use httpd::StatusCode;
use std::sync::Arc;
use std::time::Duration;
use sys::tempfile;
use the_block::governance::treasury::{mark_cancelled, mark_executed};
use the_block::governance::GovStore;
use the_block::governance::TreasuryDisbursement;

#[test]
fn treasury_index_and_filters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let scheduled = TreasuryDisbursement::new(1, "dest-1".into(), 100, "memo-1".into(), 5);
    let mut executed = TreasuryDisbursement::new(2, "dest-2".into(), 200, String::new(), 10);
    mark_executed(&mut executed, "0xabc".into());
    let mut cancelled = TreasuryDisbursement::new(3, "dest-3".into(), 150, String::new(), 2);
    mark_cancelled(&mut cancelled, "no longer required".into());

    explorer
        .index_treasury_disbursements(&[scheduled.clone(), executed.clone(), cancelled.clone()])
        .expect("index disbursements");

    let full_page = explorer
        .treasury_disbursements(0, 10, TreasuryDisbursementFilter::default())
        .expect("list all");
    assert_eq!(full_page.total, 3);
    assert_eq!(full_page.disbursements[0].id, executed.id);
    assert_eq!(full_page.disbursements[1].id, scheduled.id);
    assert_eq!(full_page.disbursements[2].id, cancelled.id);
    assert_eq!(full_page.disbursements[0].status_label, "executed");
    assert_eq!(full_page.disbursements[2].status_label, "rolled_back");
    assert_eq!(
        full_page.disbursements[0].executed_tx_hash.as_deref(),
        Some("0xabc")
    );

    let scheduled_only = explorer
        .treasury_disbursements(
            0,
            10,
            TreasuryDisbursementFilter {
                status: Some(TreasuryDisbursementStatusFilter::Draft),
                ..Default::default()
            },
        )
        .expect("scheduled filter");
    assert_eq!(scheduled_only.total, 1);
    assert_eq!(scheduled_only.disbursements[0].id, scheduled.id);

    // HTTP endpoint filtering for executed disbursements
    let app = router(ExplorerHttpState::new(explorer.clone()));
    runtime::block_on(async {
        let response = app
            .handle(
                app.request_builder()
                    .path("/governance/treasury/disbursements")
                    .query_param("status", "executed")
                    .query_param("page", "0")
                    .query_param("page_size", "5")
                    .build(),
            )
            .await
            .expect("treasury response");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: json::Value = json::from_slice(response.body()).expect("decode payload");
        let entries = payload["disbursements"].as_array().expect("array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["id"].as_u64(), Some(executed.id));
        assert_eq!(entries[0]["status_label"].as_str(), Some("executed"));
    });
}

#[test]
fn treasury_executor_lease_released_flag_exposed() {
    let gov_dir = tempfile::tempdir().expect("gov temp dir");
    let gov_path = gov_dir.path().join("gov.db");
    let gov_state = gov_path.to_string_lossy().into_owned();
    let store = GovStore::open(gov_state.clone());
    let (_lease, acquired) = store
        .refresh_executor_lease("executor-1", Duration::from_secs(60))
        .expect("acquire lease");
    assert!(acquired);
    store
        .release_executor_lease("executor-1")
        .expect("release lease");

    let report = build_executor_report(&gov_state).expect("executor report");
    assert!(report.lease_released());
}

#[test]
fn treasury_timeline_persistence_and_filters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let events = vec![
        TreasuryTimelineEntry {
            disbursement_id: 42,
            destination: "dest-42".into(),
            amount: 10,
            memo: "note".into(),
            scheduled_epoch: 5,
            tx_hash: "0xaaa".into(),
            executed_at: 100,
            block_hash: "block-a".into(),
            block_height: 10,
        },
        TreasuryTimelineEntry {
            disbursement_id: 43,
            destination: "dest-43".into(),
            amount: 20,
            memo: "note2".into(),
            scheduled_epoch: 6,
            tx_hash: "0xbbb".into(),
            executed_at: 120,
            block_hash: "block-b".into(),
            block_height: 11,
        },
    ];

    explorer
        .index_treasury_timeline_entries(&events)
        .expect("index timeline");

    let page = explorer
        .treasury_timeline(0, 5, TreasuryTimelineFilter::default())
        .expect("load timeline");
    assert_eq!(page.total, 2);
    assert_eq!(page.events[0].disbursement_id, 43);
    assert_eq!(page.events[1].disbursement_id, 42);

    // HTTP endpoint with filter
    let app = router(ExplorerHttpState::new(explorer.clone()));
    runtime::block_on(async {
        let response = app
            .handle(
                app.request_builder()
                    .path("/governance/treasury/timeline")
                    .query_param("disbursement_id", "42")
                    .build(),
            )
            .await
            .expect("timeline response");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: json::Value = json::from_slice(response.body()).expect("decode payload");
        assert_eq!(payload["total"].as_u64(), Some(1));
        let entries = payload["events"].as_array().expect("array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["disbursement_id"].as_u64(), Some(42));
    });
}
