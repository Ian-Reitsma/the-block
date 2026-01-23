use crypto_suite::hashing::blake3;
use foundation_serialization::json;
use foundation_serialization::json::Value;
use foundation_telemetry::{GovernanceWrapperEntry, WrapperMetricEntry, WrapperSummaryEntry};
use std::collections::{BTreeMap, HashMap};
use std::fs;

#[test]
fn wrappers_snapshot_hash_is_pinned() {
    let mut map: BTreeMap<String, WrapperSummaryEntry> = BTreeMap::new();
    let mut energy_shortfall_labels = HashMap::new();
    energy_shortfall_labels.insert("provider".into(), "energy-0x1".into());
    let mut energy_reject_labels = HashMap::new();
    energy_reject_labels.insert("reason".into(), "invalid_reading".into());
    let mut energy_dispute_open = HashMap::new();
    energy_dispute_open.insert("state".into(), "open".into());
    let mut energy_dispute_resolved = HashMap::new();
    energy_dispute_resolved.insert("state".into(), "resolved".into());
    let mut energy_slash_labels = HashMap::new();
    energy_slash_labels.insert("provider".into(), "energy-0x1".into());
    energy_slash_labels.insert("reason".into(), "quorum".into());
    let mut handshake_quinn_labels = HashMap::new();
    handshake_quinn_labels.insert("provider".into(), "quinn".into());
    let mut handshake_s2n_labels = HashMap::new();
    handshake_s2n_labels.insert("provider".into(), "s2n-quic".into());
    let mut blocktorch_kernel_labels = HashMap::new();
    blocktorch_kernel_labels.insert("digest".into(), "abc123-kernel".into());
    let mut blocktorch_benchmark_labels = HashMap::new();
    blocktorch_benchmark_labels.insert("commit".into(), "bench-abc123".into());
    let mut blocktorch_trace_labels = HashMap::new();
    blocktorch_trace_labels.insert("hash".into(), "trace-hash-xyz".into());
    blocktorch_trace_labels.insert("source".into(), "wrappers".into());
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
                    metric: "energy_quorum_shortfall_total".into(),
                    labels: energy_shortfall_labels.clone(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_reading_reject_total".into(),
                    labels: energy_reject_labels,
                    value: 4.0,
                },
                WrapperMetricEntry {
                    metric: "energy_dispute_total".into(),
                    labels: energy_dispute_open,
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "energy_dispute_total".into(),
                    labels: energy_dispute_resolved,
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_settlement_mode".into(),
                    labels: HashMap::new(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_settlement_rollback_total".into(),
                    labels: HashMap::new(),
                    value: 2.0,
                },
                WrapperMetricEntry {
                    metric: "energy_slashing_total".into(),
                    labels: energy_slash_labels,
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_provider_total".into(),
                    labels: HashMap::new(),
                    value: 5.0,
                },
                WrapperMetricEntry {
                    metric: "energy_pending_credits_total".into(),
                    labels: HashMap::new(),
                    value: 12.0,
                },
                WrapperMetricEntry {
                    metric: "energy_receipt_total".into(),
                    labels: HashMap::new(),
                    value: 7.0,
                },
                WrapperMetricEntry {
                    metric: "energy_active_disputes_total".into(),
                    labels: HashMap::new(),
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "energy_disputes_pending".into(),
                    labels: HashMap::new(),
                    value: 2.0,
                },
                WrapperMetricEntry {
                    metric: "energy_provider_register_total".into(),
                    labels: HashMap::new(),
                    value: 4.0,
                },
                WrapperMetricEntry {
                    metric: "energy_treasury_fee_total".into(),
                    labels: HashMap::new(),
                    value: 20.0,
                },
                WrapperMetricEntry {
                    metric: "energy_dispute_open_total".into(),
                    labels: HashMap::new(),
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "energy_dispute_resolve_total".into(),
                    labels: HashMap::new(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_settlement_mode".into(),
                    labels: HashMap::new(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "energy_settlement_rollback_total".into(),
                    labels: HashMap::new(),
                    value: 2.0,
                },
                WrapperMetricEntry {
                    metric: "energy_meter_reading_total".into(),
                    labels: energy_shortfall_labels.clone(),
                    value: 13.0,
                },
                WrapperMetricEntry {
                    metric: "receipts_compute_slash_total".into(),
                    labels: HashMap::new(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "receipts_compute_slash_per_block".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "receipt_settlement_compute_slash".into(),
                    labels: HashMap::new(),
                    value: 42.0,
                },
                WrapperMetricEntry {
                    metric: "remote_signer_discovery_total".into(),
                    labels: HashMap::new(),
                    value: 5.0,
                },
                WrapperMetricEntry {
                    metric: "remote_signer_discovery_success_total".into(),
                    labels: HashMap::new(),
                    value: 2.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_forwarder_retry_total".into(),
                    labels: HashMap::new(),
                    value: 8.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_forwarder_drop_total".into(),
                    labels: HashMap::new(),
                    value: 2.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_forwarder_fail_total".into(),
                    labels: HashMap::new(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_enqueue_error_total".into(),
                    labels: HashMap::new(),
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_queue_depth".into(),
                    labels: HashMap::new(),
                    value: 12.0,
                },
                WrapperMetricEntry {
                    metric: "range_boost_queue_oldest_seconds".into(),
                    labels: HashMap::new(),
                    value: 34.0,
                },
                WrapperMetricEntry {
                    metric: "localnet_receipt_insert_attempt_total".into(),
                    labels: HashMap::new(),
                    value: 15.0,
                },
                WrapperMetricEntry {
                    metric: "localnet_receipt_insert_success_total".into(),
                    labels: HashMap::new(),
                    value: 12.0,
                },
                WrapperMetricEntry {
                    metric: "localnet_receipt_insert_failure_total".into(),
                    labels: HashMap::new(),
                    value: 3.0,
                },
                WrapperMetricEntry {
                    metric: "blocktorch_kernel_variant_digest".into(),
                    labels: blocktorch_kernel_labels.clone(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "blocktorch_benchmark_commit".into(),
                    labels: blocktorch_benchmark_labels.clone(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "blocktorch_proof_latency_ms".into(),
                    labels: HashMap::new(),
                    value: 42.5,
                },
                WrapperMetricEntry {
                    metric: "blocktorch_aggregator_trace".into(),
                    labels: blocktorch_trace_labels.clone(),
                    value: 1.0,
                },
                WrapperMetricEntry {
                    metric: "receipt_drain_depth".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "proof_verification_latency_ms".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "sla_breach_depth".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "orchard_alloc_free_delta".into(),
                    labels: HashMap::new(),
                    value: 0.0,
                },
                WrapperMetricEntry {
                    metric: "transport_handshake_attempt_total".into(),
                    labels: handshake_quinn_labels.clone(),
                    value: 18.0,
                },
                WrapperMetricEntry {
                    metric: "transport_handshake_attempt_total".into(),
                    labels: handshake_s2n_labels.clone(),
                    value: 5.0,
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
    let value = json::to_value(&map).expect("serialize wrappers map");
    let encoded = json::to_vec_value(&value);
    if std::env::var("PRINT_WRAPPERS_SNAPSHOT").as_deref() == Ok("1") {
        let serialized =
            String::from_utf8(encoded.clone()).expect("wrappers map utf8 serialization");
        eprintln!("{serialized}");
    }
    if std::env::var("WRITE_WRAPPERS_SNAPSHOT").as_deref() == Ok("1") {
        fs::write("tests/snapshots/wrappers.json", &encoded)
            .expect("write canonical wrappers snapshot");
    }

    let hash = blake3::hash(&encoded).to_hex().to_string();
    assert_eq!(
        hash.as_str(),
        "5948fdf91fa8b7f47ea70607eaab03ec00f66b25caf717ec86a34cec1cc200ba",
        "wrappers schema or field set drifted; refresh snapshot intentionally (current {})",
        hash
    );

    let snapshot = fs::read("tests/snapshots/wrappers.json")
        .expect("wrappers snapshot file present (generate via WRITE_WRAPPERS_SNAPSHOT=1 cargo test -p monitoring --test wrappers)");
    assert_eq!(
        snapshot, encoded,
        "wrappers snapshot drifted from canonical encoding; regenerate with WRITE_WRAPPERS_SNAPSHOT=1"
    );

    let snapshot_value: Value =
        json::from_slice(&snapshot).expect("wrappers snapshot json deserializes");
    let obj = snapshot_value
        .as_object()
        .expect("wrappers snapshot object");
    assert!(
        obj.contains_key("node-a"),
        "expected node-a entry to anchor governance wrappers hash"
    );
    let governance = obj
        .get("node-a")
        .and_then(|entry| entry.get("governance"))
        .and_then(|gov| gov.as_object())
        .expect("governance wrapper present");
    assert!(
        governance.contains_key("treasury_balance"),
        "governance wrapper missing treasury_balance"
    );
}
