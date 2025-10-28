use explorer::{
    router, Explorer, ExplorerHttpState, TreasuryDisbursementFilter,
    TreasuryDisbursementStatusFilter,
};
use foundation_serialization::json;
use httpd::StatusCode;
use std::sync::Arc;
use sys::tempfile;
use the_block::governance::treasury::{mark_cancelled, mark_executed};
use the_block::governance::TreasuryDisbursement;

#[test]
fn treasury_index_and_filters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let scheduled = TreasuryDisbursement::new(1, "dest-1".into(), 100, 25, "memo-1".into(), 5);
    let mut executed = TreasuryDisbursement::new(2, "dest-2".into(), 200, 50, String::new(), 10);
    mark_executed(&mut executed, "0xabc".into());
    let mut cancelled = TreasuryDisbursement::new(3, "dest-3".into(), 150, 75, String::new(), 2);
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
    assert_eq!(
        full_page.disbursements[0].executed_tx_hash.as_deref(),
        Some("0xabc")
    );

    let scheduled_only = explorer
        .treasury_disbursements(
            0,
            10,
            TreasuryDisbursementFilter {
                status: Some(TreasuryDisbursementStatusFilter::Scheduled),
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
