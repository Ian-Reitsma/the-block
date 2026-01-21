use explorer::{
    build_executor_report, router, Explorer, ExplorerHttpState, TreasuryDisbursementFilter,
    TreasuryDisbursementStatusFilter, TreasuryTimelineEntry, TreasuryTimelineFilter,
};
use crypto_suite::hashing::blake3;
use foundation_serialization::json;
use httpd::StatusCode;
use std::sync::Arc;
use std::time::Duration;
use sys::tempfile;
use the_block::governance::treasury::{
    mark_cancelled, mark_executed, DisbursementDetails, DisbursementPayload,
    DisbursementProposalMetadata, TreasuryDisbursement, MAX_DEPENDENCIES, MAX_MEMO_BYTES,
};
use the_block::governance::GovStore;

#[test]
fn treasury_index_and_filters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let scheduled = TreasuryDisbursement::new(1, "tb1dest-1".into(), 100, "memo-1".into(), 5);
    let mut executed = TreasuryDisbursement::new(2, "tb1dest-2".into(), 200, String::new(), 10);
    mark_executed(&mut executed, "0xabc".into());
    let mut cancelled = TreasuryDisbursement::new(3, "tb1dest-3".into(), 150, String::new(), 2);
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
fn treasury_dependency_limits_clamp_explorer_payloads() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let heavy_payload = DisbursementPayload {
        proposal: DisbursementProposalMetadata {
            deps: (1..=(MAX_DEPENDENCIES as u64 + 25)).collect(),
            ..Default::default()
        },
        disbursement: DisbursementDetails {
            destination: "tb1heavy".into(),
            amount: 50,
            memo: "proposal-deps".into(),
            scheduled_epoch: 10,
            expected_receipts: Vec::new(),
        },
    };
    let heavy = TreasuryDisbursement::from_payload(10, heavy_payload);

    let memo_deps: Vec<String> = (1..=(MAX_DEPENDENCIES as u64 + 30))
        .map(|id| id.to_string())
        .collect();
    let memo_payload = DisbursementPayload {
        proposal: DisbursementProposalMetadata::default(),
        disbursement: DisbursementDetails {
            destination: "tb1memo".into(),
            amount: 75,
            memo: format!("depends_on={}", memo_deps.join(",")),
            scheduled_epoch: 11,
            expected_receipts: Vec::new(),
        },
    };
    let memo_based = TreasuryDisbursement::from_payload(11, memo_payload);

    explorer
        .index_treasury_disbursements(&[heavy.clone(), memo_based.clone()])
        .expect("index disbursements with dependencies");

    let page = explorer
        .treasury_disbursements(0, 10, TreasuryDisbursementFilter::default())
        .expect("load clamped deps");

    let heavy_row = page
        .disbursements
        .iter()
        .find(|d| d.id == heavy.id)
        .expect("heavy disbursement present");
    assert_eq!(heavy_row.deps.len(), MAX_DEPENDENCIES);
    assert_eq!(heavy_row.deps[0], 1);
    assert_eq!(
        heavy_row.deps[MAX_DEPENDENCIES - 1],
        MAX_DEPENDENCIES as u64
    );

    let memo_row = page
        .disbursements
        .iter()
        .find(|d| d.id == memo_based.id)
        .expect("memo disbursement present");
    assert_eq!(memo_row.deps.len(), MAX_DEPENDENCIES);
    assert!(memo_row.memo.starts_with("depends_on="));
    assert_eq!(
        memo_row.deps[MAX_DEPENDENCIES - 1],
        MAX_DEPENDENCIES as u64
    );
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

    let long_memo = format!("memo-{}{}", "x".repeat(MAX_MEMO_BYTES), "overflow");
    let events = vec![
        TreasuryTimelineEntry {
            disbursement_id: 42,
            destination: "tb1dest-42".into(),
            amount: 10,
            memo: long_memo.clone(),
            scheduled_epoch: 5,
            tx_hash: "0xaaa".into(),
            executed_at: 100,
            block_hash: "block-a".into(),
            block_height: 10,
        },
        TreasuryTimelineEntry {
            disbursement_id: 43,
            destination: "tb1dest-43".into(),
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
    assert!(
        page.events[1].memo.len() <= MAX_MEMO_BYTES,
        "memo should be clamped to MAX_MEMO_BYTES"
    );
    assert_eq!(page.events[1].memo.len(), MAX_MEMO_BYTES);

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
        assert_eq!(
            entries[0]["memo"]
                .as_str()
                .map(|m| m.len())
                .expect("memo string length"),
            MAX_MEMO_BYTES
        );
    });
}

#[test]
fn treasury_http_dep_limit_and_schema_hash_guard() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

    let heavy_payload = DisbursementPayload {
        proposal: DisbursementProposalMetadata {
            deps: (1..=MAX_DEPENDENCIES as u64 + 5).collect(),
            ..Default::default()
        },
        disbursement: DisbursementDetails {
            destination: "tb1deps-http".into(),
            amount: 10,
            memo: "{}".into(),
            scheduled_epoch: 1,
            expected_receipts: Vec::new(),
        },
    };
    let heavy = TreasuryDisbursement::from_payload(7, heavy_payload);
    explorer
        .index_treasury_disbursements(&[heavy.clone()])
        .expect("index http disbursements");

    let app = router(ExplorerHttpState::new(explorer.clone()));
    runtime::block_on(async {
        let response = app
            .handle(
                app.request_builder()
                    .path("/governance/treasury/disbursements")
                    .query_param("page", "0")
                    .query_param("page_size", "5")
                    .build(),
            )
            .await
            .expect("treasury http response");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: json::Value = json::from_slice(response.body()).expect("decode payload");
        let entries = payload["disbursements"]
            .as_array()
            .expect("entries array");
        assert_eq!(entries.len(), 1);
        let deps = entries[0]["deps"]
            .as_array()
            .expect("deps array in http payload");
        assert_eq!(deps.len(), MAX_DEPENDENCIES);

        // Schema guard: key set should remain stable so CLI/SDK hashes stay aligned.
        let mut keys: Vec<String> = entries[0]
            .as_object()
            .expect("disbursement object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        let required_keys = vec![
            "amount",
            "created_at",
            "deps",
            "destination",
            "id",
            "memo",
            "scheduled_epoch",
            "status",
            "status_label",
            "status_timestamp",
        ];
        for required in &required_keys {
            assert!(
                keys.contains(&required.to_string()),
                "missing required key {required} in treasury disbursement schema"
            );
        }
        let allowed_keys = vec![
            "amount",
            "cancel_reason",
            "created_at",
            "deps",
            "destination",
            "executed_tx_hash",
            "expected_receipts",
            "id",
            "memo",
            "scheduled_epoch",
            "status",
            "status_label",
            "status_timestamp",
        ];
        let allowed_set: std::collections::HashSet<_> =
            allowed_keys.iter().map(|k| k.to_string()).collect();
        let actual_set: std::collections::HashSet<_> = keys.iter().cloned().collect();
        assert!(
            actual_set.is_subset(&allowed_set),
            "unexpected key drift in treasury schema: {:?}",
            keys
        );
        let schema_hash = blake3::hash(allowed_keys.join("|").as_bytes());
        let schema_hex = schema_hash.to_hex().to_string();
        assert_eq!(
            schema_hex.as_str(),
            "c48f401c3792195c9010024b8ba0269b0efd56c227be9cb5dd1ddba793b2cbd1",
            "schema hash mismatch; update CLI/SDK fixtures and allowed_keys if this is intentional"
        );
    });
}
