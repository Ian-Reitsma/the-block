use crypto_suite::hashing::blake3;
use foundation_serialization::json;
use foundation_serialization::json::Value;
use foundation_telemetry::{GovernanceWrapperEntry, WrapperMetricEntry, WrapperSummaryEntry};
use std::collections::{BTreeMap, HashMap};
use std::fs;

#[test]
fn wrappers_snapshot_hash_is_pinned() {
    let mut map: BTreeMap<String, WrapperSummaryEntry> = BTreeMap::new();
    map.insert(
        "node-a".into(),
        WrapperSummaryEntry {
            metrics: vec![WrapperMetricEntry {
                metric: "governance.treasury.executor.last_submitted_nonce".into(),
                labels: HashMap::new(),
                value: 7.0,
            }],
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
        "e6982a8b84b28b043f1470eafbb8ae77d12e79a9059e21eec518beeb03566595",
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

