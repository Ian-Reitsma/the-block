use contract_cli::gov::{
    combine_treasury_fetch_results, handle_with_writer, treasury_disbursement_params,
    treasury_history_params, GovCmd, GovTreasuryCmd, RemoteTreasuryStatus,
    RpcTreasuryBalanceResult, RpcTreasuryDisbursementsResult, RpcTreasuryHistoryResult,
    TreasuryDisbursementQuery,
};
use foundation_serialization::json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use governance::{
    DisbursementStatus, GovStore, TreasuryBalanceEventKind, TreasuryBalanceSnapshot,
    TreasuryDisbursement,
};
use sys::tempfile;

fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.to_owned())
}

fn json_number_u64(value: u64) -> JsonValue {
    JsonValue::Number(JsonNumber::from(value))
}

fn json_object(entries: impl IntoIterator<Item = (&'static str, JsonValue)>) -> JsonValue {
    let mut map = JsonMap::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    JsonValue::Object(map)
}

#[test]
fn treasury_lifecycle_outputs_structured_json() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_path = dir.path().join("gov.db");
    let state = state_path.to_string_lossy().into_owned();

    let mut out = Vec::new();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Schedule {
                destination: "dest-1".into(),
                amount: 500,
                amount_it: 0,
                memo: Some("ecosystem grant".into()),
                epoch: 2048,
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("schedule disbursement");
    let scheduled = fetch_disbursement(&state, 1);
    assert_eq!(scheduled.destination, "dest-1");
    assert_eq!(scheduled.amount_ct, 500);
    assert!(matches!(scheduled.status, DisbursementStatus::Scheduled));
    let first_created_at = scheduled.created_at;

    out.clear();
    handle_with_writer(
        GovCmd::Treasury {
            action: GovTreasuryCmd::Schedule {
                destination: "dest-2".into(),
                amount: 200,
                amount_it: 0,
                memo: None,
                epoch: 4096,
                state: state.clone(),
            },
        },
        &mut out,
    )
    .expect("schedule second disbursement");
    let queued_second = fetch_disbursement(&state, 2);
    assert_eq!(queued_second.destination, "dest-2");
    assert!(matches!(
        queued_second.status,
        DisbursementStatus::Scheduled
    ));

    let store = GovStore::open(state.clone());
    store
        .record_treasury_accrual(1_000, 0)
        .expect("fund treasury before execution");

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
    let executed = fetch_disbursement(&state, 1);
    match executed.status {
        DisbursementStatus::Executed {
            executed_at,
            ref tx_hash,
        } => {
            assert!(executed_at >= first_created_at);
            assert_eq!(tx_hash, "0xfeed");
        }
        other => panic!("unexpected status after execute: {other:?}"),
    }

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
    let cancelled = fetch_disbursement(&state, 2);
    match cancelled.status {
        DisbursementStatus::Cancelled {
            cancelled_at,
            ref reason,
        } => {
            assert!(cancelled_at >= queued_second.created_at);
            assert_eq!(reason, "policy update");
        }
        other => panic!("unexpected status after cancel: {other:?}"),
    }

    let store = GovStore::open(state.clone());
    let entries = store.disbursements().expect("disbursements");
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .any(|entry| matches!(entry.status, DisbursementStatus::Executed { .. })));
    assert!(entries
        .iter()
        .any(|entry| matches!(entry.status, DisbursementStatus::Cancelled { .. })));
}

#[test]
fn treasury_fetch_remote_combines_responses() {
    let mut query = TreasuryDisbursementQuery::default();
    query.status = Some(RemoteTreasuryStatus::Scheduled);
    query.after_id = Some(3);
    query.limit = Some(4);
    let disbursement_params = treasury_disbursement_params(&query);
    let expected_disb_params = json_object([
        ("status", json_string("scheduled")),
        ("after_id", json_number_u64(3)),
        ("limit", json_number_u64(4)),
    ]);
    assert_eq!(disbursement_params, expected_disb_params);

    let history_params = treasury_history_params(Some(2), Some(5));
    let expected_history_params = json_object([
        ("after_id", json_number_u64(2)),
        ("limit", json_number_u64(5)),
    ]);
    assert_eq!(history_params, expected_history_params);

    let disbursement_result = RpcTreasuryDisbursementsResult {
        disbursements: vec![TreasuryDisbursement {
            id: 7,
            destination: "remote-dest".into(),
            amount_ct: 320,
            amount_it: 45,
            memo: "ops".into(),
            scheduled_epoch: 9000,
            created_at: 1_700_000_000,
            status: DisbursementStatus::Scheduled,
        }],
        next_cursor: Some(12),
    };
    let balance_result = RpcTreasuryBalanceResult {
        balance_ct: 4_400,
        balance_it: 1_050,
        last_snapshot: Some(TreasuryBalanceSnapshot {
            id: 5,
            balance_ct: 4_400,
            delta_ct: 200,
            balance_it: 1_050,
            delta_it: 80,
            recorded_at: 1_700_000_100,
            event: TreasuryBalanceEventKind::Accrual,
            disbursement_id: None,
        }),
    };
    let history_result = RpcTreasuryHistoryResult {
        snapshots: vec![TreasuryBalanceSnapshot {
            id: 6,
            balance_ct: 4_400,
            delta_ct: -120,
            balance_it: 970,
            delta_it: -90,
            recorded_at: 1_700_000_200,
            event: TreasuryBalanceEventKind::Executed,
            disbursement_id: Some(4),
        }],
        next_cursor: None,
        current_balance_ct: 4_400,
        current_balance_it: 1_050,
    };

    let output =
        combine_treasury_fetch_results(disbursement_result, balance_result, Some(history_result));
    assert_eq!(output.balance_ct, 4_400);
    assert_eq!(output.balance_it, 1_050);
    assert_eq!(output.next_cursor, Some(12));
    assert!(output
        .balance_history
        .as_ref()
        .map(|history| !history.is_empty())
        .unwrap_or(false));

    assert_eq!(output.disbursements.len(), 1);
    assert!(matches!(
        output
            .disbursements
            .first()
            .map(|entry| entry.status.clone()),
        Some(DisbursementStatus::Scheduled)
    ));
    let history = output
        .balance_history
        .as_ref()
        .expect("history included in combined result");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].delta_ct, -120);
    assert_eq!(history[0].delta_it, -90);
}

#[test]
fn treasury_fetch_remote_allows_missing_history() {
    let disbursement_result = RpcTreasuryDisbursementsResult {
        disbursements: vec![],
        next_cursor: None,
    };
    let balance_result = RpcTreasuryBalanceResult {
        balance_ct: 0,
        balance_it: 0,
        last_snapshot: None,
    };

    let output = combine_treasury_fetch_results(disbursement_result, balance_result, None);
    assert_eq!(output.balance_ct, 0);
    assert_eq!(output.balance_it, 0);
    assert_eq!(output.next_cursor, None);
    assert!(output.balance_history.is_none());
    assert!(output.disbursements.is_empty());
}

fn fetch_disbursement(state: &str, id: u64) -> TreasuryDisbursement {
    let store = GovStore::open(state.to_string());
    let records = store.disbursements().expect("disbursements");
    records
        .into_iter()
        .find(|record| record.id == id)
        .expect("disbursement record")
}
