use storage_market::slashing::{
    Config, ReceiptMetadata, RepairKey, RepairReport, SlashingAuditEvent, SlashingController,
    SlashingReason,
};

fn receipt_meta(
    provider: &str,
    nonce: u64,
    block_height: u64,
    region: Option<&str>,
    chunk_hash: Option<[u8; 32]>,
) -> ReceiptMetadata {
    ReceiptMetadata {
        provider: provider.to_string(),
        signature_nonce: nonce,
        block_height,
        contract_id: "contract-1".to_string(),
        region: region.map(str::to_string),
        chunk_hash,
    }
}

fn repair_key(contract: &str, provider: &str) -> RepairKey {
    RepairKey {
        contract_id: contract.to_string(),
        provider: provider.to_string(),
        chunk_hash: [0u8; 32],
    }
}

#[test]
fn missing_repairs_emit_slash() {
    let config = Config {
        repair_window: 4,
        dark_threshold: 8,
    };
    let mut controller = SlashingController::new(config);
    let key = repair_key("contract-1", "provider-a");
    controller.report_missing_chunk(RepairReport {
        key: key.clone(),
        block_height: 10,
        missing_bytes: 50,
        provider_escrow: 20,
        rent_per_byte: 2,
        region: Some("us-east".into()),
    });

    let slashes = controller.drain_slashes(15);
    assert_eq!(slashes.len(), 1);
    let slash = &slashes[0];
    assert_eq!(slash.provider, key.provider);
    assert_eq!(slash.region.as_deref(), Some("us-east"));
    assert_eq!(
        slash.reason,
        SlashingReason::MissingRepair {
            contract_id: key.contract_id.clone(),
            chunk_hash: key.chunk_hash,
        }
    );
    assert_eq!(slash.amount, 100); // rent_per_byte * missing_bytes
}

#[test]
fn replayed_nonces_trigger_slash() {
    let mut controller = SlashingController::new(Config::default());

    let first = controller.record_receipt(receipt_meta("provider-b", 42, 5, Some("us-west"), None));
    assert!(first.is_empty());

    let second =
        controller.record_receipt(receipt_meta("provider-b", 42, 9, Some("us-west"), None));
    assert_eq!(second.len(), 1);
    assert!(matches!(
        second[0].reason,
        SlashingReason::ReplayedNonce { nonce: 42 }
    ));
}

#[test]
fn dark_region_reroutes_mark_dark() {
    let config = Config {
        repair_window: 10,
        dark_threshold: 3,
    };
    let mut controller = SlashingController::new(config);
    controller.record_receipt(receipt_meta("provider-c", 1, 7, Some("eu-central"), None));

    let slashes = controller.drain_slashes(11);
    assert!(slashes.iter().any(|slash| matches!(
        &slash.reason,
        storage_market::slashing::SlashingReason::RegionDark { region }
        if region == "eu-central"
    )));
    let status = controller
        .region_status("eu-central")
        .expect("region tracked");
    assert!(status.is_dark());
    assert_eq!(status.dark_since, Some(11));
}

#[test]
fn colluding_providers_duplicate_nonce_slash_all() {
    let mut controller = SlashingController::new(Config::default());
    let chunk_hash = [42u8; 32];
    controller.record_receipt(receipt_meta(
        "provider-a",
        12,
        20,
        Some("us-west"),
        Some(chunk_hash),
    ));

    let slashes = controller.record_receipt(receipt_meta(
        "provider-b",
        12,
        21,
        Some("us-west"),
        Some(chunk_hash),
    ));

    assert_eq!(slashes.len(), 2);
    let providers: std::collections::HashSet<_> = slashes
        .iter()
        .map(|slash| slash.provider.as_str())
        .collect();
    assert!(providers.contains("provider-a"));
    assert!(providers.contains("provider-b"));
    assert!(slashes
        .iter()
        .all(|slash| matches!(slash.reason, SlashingReason::ReplayedNonce { nonce: 12 })));
}

#[test]
fn repair_deadline_clears_when_chunk_returns() {
    let config = Config {
        repair_window: 4,
        dark_threshold: 20,
    };
    let mut controller = SlashingController::new(config);
    let key = repair_key("contract-1", "provider-d");
    controller.report_missing_chunk(RepairReport {
        key: key.clone(),
        block_height: 100,
        missing_bytes: 8,
        provider_escrow: 15,
        rent_per_byte: 3,
        region: Some("ap-south".into()),
    });

    controller.record_receipt(receipt_meta(
        "provider-d",
        99,
        102,
        Some("ap-south"),
        Some(key.chunk_hash),
    ));

    let slashes = controller.drain_slashes(110);
    assert!(slashes.is_empty());
}

#[test]
fn missing_chunk_flag_prevents_payment_until_repaired() {
    let config = Config {
        repair_window: 6,
        dark_threshold: 20,
    };
    let mut controller = SlashingController::new(config);
    let key = repair_key("contract-42", "provider-e");
    controller.report_missing_chunk(RepairReport {
        key: key.clone(),
        block_height: 5,
        missing_bytes: 16,
        provider_escrow: 40,
        rent_per_byte: 3,
        region: Some("sa-east".into()),
    });

    assert!(controller.is_chunk_missing(&key));
    assert_eq!(controller.repair_deadline(&key), Some(11));

    controller.record_receipt(receipt_meta(
        "provider-e",
        1,
        10,
        Some("sa-east"),
        Some(key.chunk_hash),
    ));

    assert!(!controller.is_chunk_missing(&key));
    assert!(controller.audit_log().iter().any(|entry| {
        matches!(
            &entry.event,
            SlashingAuditEvent::RepairCleared { key: cleared, .. } if cleared == &key
        )
    }));
}

#[test]
fn duplicate_nonce_emits_audit_event() {
    let mut controller = SlashingController::new(Config::default());
    let chunk_hash = [9u8; 32];
    controller.record_receipt(receipt_meta(
        "provider-a",
        99,
        10,
        Some("us-central"),
        Some(chunk_hash),
    ));

    controller.record_receipt(receipt_meta(
        "provider-b",
        99,
        11,
        Some("us-central"),
        Some(chunk_hash),
    ));

    assert!(controller.audit_log().iter().any(|entry| {
        matches!(
            &entry.event,
            SlashingAuditEvent::DuplicateNonce {
                contract_id,
                nonce,
                providers,
            } if *nonce == 99 && contract_id == "contract-1" && providers.len() == 2
        )
    }));
}
