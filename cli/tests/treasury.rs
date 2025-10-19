mod support;

use contract_cli::gov::{handle_with_writer, GovCmd, GovTreasuryCmd, RemoteTreasuryStatus};
use foundation_serialization::json;
use governance::{DisbursementStatus, TreasuryDisbursement};
use support::json_rpc::JsonRpcMock;
use sys::tempfile;

#[test]
fn treasury_lifecycle_outputs_structured_json() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_path = dir.path().join("gov.db");
    let state = state_path.to_string_lossy().into_owned();

    // Schedule first disbursement
    let mut out = Vec::new();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Schedule {
                destination: "dest-1".into(),
                amount: 500,
                memo: Some("ecosystem grant".into()),
                epoch: 2048,
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("schedule disbursement");
    let scheduled: TreasuryDisbursement = json::from_slice(&out).expect("schedule json");
    assert_eq!(scheduled.id, 1);
    assert_eq!(scheduled.destination, "dest-1");
    assert!(matches!(scheduled.status, DisbursementStatus::Scheduled));

    // Schedule second disbursement for cancellation later
    out.clear();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Schedule {
                destination: "dest-2".into(),
                amount: 200,
                memo: None,
                epoch: 4096,
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("schedule second disbursement");
    let queued_second: TreasuryDisbursement = json::from_slice(&out).expect("second schedule json");
    assert_eq!(queued_second.id, 2);

    // Execute first disbursement
    out.clear();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Execute {
                id: scheduled.id,
                tx_hash: "0xfeed".into(),
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("execute disbursement");
    let executed: TreasuryDisbursement = json::from_slice(&out).expect("execute json");
    match executed.status {
        DisbursementStatus::Executed {
            executed_at,
            ref tx_hash,
        } => {
            assert!(executed_at >= scheduled.created_at);
            assert_eq!(tx_hash, "0xfeed");
        }
        other => panic!("unexpected status after execute: {other:?}"),
    }

    // Cancel the second disbursement
    out.clear();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Cancel {
                id: queued_second.id,
                reason: "policy update".into(),
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("cancel disbursement");
    let cancelled: TreasuryDisbursement = json::from_slice(&out).expect("cancel json");
    match cancelled.status {
        DisbursementStatus::Cancelled {
            cancelled_at,
            ref reason,
        } => {
            assert!(cancelled_at >= cancelled.created_at);
            assert_eq!(reason, "policy update");
        }
        other => panic!("unexpected status after cancel: {other:?}"),
    }

    // List disbursements should include both entries with their terminal states
    out.clear();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::List {
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("list disbursements");
    let payload: json::Value = json::from_slice(&out).expect("list json");
    let entries = payload["disbursements"]
        .as_array()
        .expect("disbursement array");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry["id"].as_u64() == Some(1)
        && entry["status"]
            .as_object()
            .map(|obj| obj.get("Executed").is_some())
            .unwrap_or(false)));
    assert!(entries.iter().any(|entry| entry["id"].as_u64() == Some(2)
        && entry["status"]
            .as_object()
            .map(|obj| obj.get("Cancelled").is_some())
            .unwrap_or(false)));
}

#[test]
fn treasury_fetch_remote_combines_responses() {
    let disbursement_payload = json::json!({
        "jsonrpc": "2.0",
        "result": {
            "disbursements": [
                {
                    "id": 7,
                    "destination": "remote-dest",
                    "amount_ct": 320,
                    "memo": "ops",
                    "scheduled_epoch": 9000,
                    "created_at": 1700000000,
                    "status": "Scheduled"
                }
            ],
            "next_cursor": 12
        },
        "id": 1
    });
    let balance_payload = json::json!({
        "jsonrpc": "2.0",
        "result": {
            "balance_ct": 4400,
            "last_snapshot": {
                "id": 5,
                "balance_ct": 4400,
                "delta_ct": 200,
                "recorded_at": 1700000100,
                "event": "Accrual"
            }
        },
        "id": 1
    });
    let history_payload = json::json!({
        "jsonrpc": "2.0",
        "result": {
            "snapshots": [
                {
                    "id": 6,
                    "balance_ct": 4400,
                    "delta_ct": -120,
                    "recorded_at": 1700000200,
                    "event": "Executed",
                    "disbursement_id": 4
                }
            ],
            "next_cursor": null,
            "current_balance_ct": 4400
        },
        "id": 1
    });
    let server = JsonRpcMock::start(vec![
        json::to_string(&disbursement_payload).expect("disbursements"),
        json::to_string(&balance_payload).expect("balance"),
        json::to_string(&history_payload).expect("history"),
    ]);

    let mut out = Vec::new();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Fetch {
                rpc: server.url().to_string(),
                status: Some(RemoteTreasuryStatus::Scheduled),
                after_id: Some(3),
                limit: Some(4),
                include_history: true,
                history_after_id: Some(2),
                history_limit: Some(5),
            },
        },
        &mut out,
    )
    .expect("fetch treasury");

    let payload: json::Value = json::from_slice(&out).expect("fetch json");
    assert_eq!(payload["balance_ct"].as_u64(), Some(4400));
    assert_eq!(
        payload["disbursements"].as_array().map(|arr| arr.len()),
        Some(1)
    );
    assert_eq!(payload["next_cursor"].as_u64(), Some(12));
    assert!(payload["balance_history"].is_array());

    let captured = server.captured();
    assert_eq!(captured.len(), 3);
    let first: json::Value = json::from_str(&captured[0]).expect("first request");
    assert_eq!(first["method"].as_str(), Some("gov.treasury.disbursements"));
    assert_eq!(
        first["params"]["status"].as_str(),
        Some(RemoteTreasuryStatus::Scheduled.as_str())
    );
    assert_eq!(first["params"]["after_id"].as_u64(), Some(3));
    assert_eq!(first["params"]["limit"].as_u64(), Some(4));

    let third: json::Value = json::from_str(&captured[2]).expect("third request");
    assert_eq!(
        third["method"].as_str(),
        Some("gov.treasury.balance_history")
    );
    assert_eq!(third["params"]["after_id"].as_u64(), Some(2));
    assert_eq!(third["params"]["limit"].as_u64(), Some(5));
}
