use storage_market::slashing::{
    Config, ReceiptMetadata, RepairKey, RepairReport, SlashingController, SlashingReason,
};

fn receipt_meta(
    provider: &str,
    nonce: u64,
    block_height: u64,
    region: Option<&str>,
) -> ReceiptMetadata {
    ReceiptMetadata {
        provider: provider.to_string(),
        signature_nonce: nonce,
        block_height,
        contract_id: "contract-1".to_string(),
        region: region.map(str::to_string),
        chunk_hash: None,
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

    let first = controller.record_receipt(receipt_meta("provider-b", 42, 5, Some("us-west")));
    assert!(first.is_empty());

    let second = controller.record_receipt(receipt_meta("provider-b", 42, 9, Some("us-west")));
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
    controller.record_receipt(receipt_meta("provider-c", 1, 7, Some("eu-central")));

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
