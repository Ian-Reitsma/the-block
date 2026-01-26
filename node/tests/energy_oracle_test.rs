#![forbid(unsafe_code)]

use crypto_suite::hashing::blake3::Hasher as Blake3;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::SigningKey;
use energy_market::{EnergyMarketError, MeterReading};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use sys::tempfile::tempdir;
use testkit::tb_serial;
use the_block::energy::{
    configure_provider_keys, flag_dispute, register_provider, resolve_dispute,
    settle_energy_delivery, slashes_page, submit_meter_reading, DisputeStatus,
    GovernanceEnergyParams, ProviderKeyConfig,
};
use the_block::governance::{EnergySettlementMode, EnergySettlementPayload};
use the_block::market_gates::{self, MarketMode};

fn sign_meter_reading(
    sk: &SigningKey,
    provider_id: &str,
    meter_address: &str,
    total_kwh: u64,
    timestamp: u64,
    nonce: u64,
) -> Vec<u8> {
    let mut hasher = Blake3::new();
    hasher.update(provider_id.as_bytes());
    hasher.update(meter_address.as_bytes());
    hasher.update(&total_kwh.to_le_bytes());
    hasher.update(&timestamp.to_le_bytes());
    hasher.update(&nonce.to_le_bytes());
    let msg = hasher.finalize();
    sk.sign(msg.as_bytes()).to_bytes().to_vec()
}

#[tb_serial]
fn energy_oracle_enforcement_and_disputes() {
    market_gates::set_energy_mode(MarketMode::Trade);
    let dir = tempdir().expect("temp dir");
    env::set_var("TB_ENERGY_MARKET_DIR", dir.path());

    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let verifying = signing.verifying_key();
    let meter_address = "meter-1".to_string();

    let params = GovernanceEnergyParams {
        min_stake: 1_000,
        oracle_timeout_blocks: 5,
        slashing_rate_bps: 0,
        settlement: EnergySettlementPayload {
            mode: EnergySettlementMode::Batch,
            quorum_threshold_ppm: 500_000,
            expiry_blocks: 5,
        },
    };
    the_block::energy::set_governance_params(params);

    let snapshot = the_block::energy::market_snapshot();
    assert_eq!(
        snapshot.governance.settlement.mode,
        EnergySettlementMode::Batch
    );
    assert_eq!(snapshot.governance.settlement.quorum_threshold_ppm, 500_000);
    assert_eq!(snapshot.governance.settlement.expiry_blocks, 5);

    let provider = register_provider(
        "owner-1".into(),
        1_000,
        2,
        meter_address.clone(),
        "US_CA".into(),
        params.min_stake,
    )
    .expect("register provider");

    configure_provider_keys(&[ProviderKeyConfig {
        provider_id: provider.provider_id.clone(),
        public_key_hex: hex::encode(verifying.to_bytes()),
    }])
    .expect("keys configured");

    let base_ts: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let signature = sign_meter_reading(
        &signing,
        &provider.provider_id,
        &meter_address,
        1_000,
        base_ts,
        1,
    );
    let credit = submit_meter_reading(
        MeterReading {
            provider_id: provider.provider_id.clone(),
            meter_address: meter_address.clone(),
            total_kwh: 1_000,
            timestamp: base_ts,
            nonce: 1,
            signature,
        },
        base_ts,
    )
    .expect("valid reading accepted");

    // Stale timestamp rejected
    let stale_err = submit_meter_reading(
        MeterReading {
            provider_id: provider.provider_id.clone(),
            meter_address: meter_address.clone(),
            total_kwh: 1_200,
            timestamp: base_ts - 1,
            nonce: 2,
            signature: sign_meter_reading(
                &signing,
                &provider.provider_id,
                &meter_address,
                1_200,
                base_ts - 1,
                2,
            ),
        },
        base_ts + 1,
    )
    .expect_err("stale reading must be rejected");
    assert!(matches!(stale_err, EnergyMarketError::StaleReading { .. }));

    // Meter regression rejected
    let regression_err = submit_meter_reading(
        MeterReading {
            provider_id: provider.provider_id.clone(),
            meter_address: meter_address.clone(),
            total_kwh: 900,
            timestamp: base_ts + 2,
            nonce: 3,
            signature: sign_meter_reading(
                &signing,
                &provider.provider_id,
                &meter_address,
                900,
                base_ts + 2,
                3,
            ),
        },
        base_ts + 2,
    )
    .expect_err("regression rejected");
    assert!(matches!(
        regression_err,
        EnergyMarketError::InvalidMeterValue { .. }
    ));

    // Batch mode blocks early settlement
    let not_due_block = base_ts + 2;
    let not_due = settle_energy_delivery(
        "buyer-1".into(),
        &provider.provider_id,
        50,
        not_due_block,
        credit.meter_reading_hash,
    )
    .expect_err("early settlement blocked");
    assert!(matches!(
        not_due,
        EnergyMarketError::SettlementNotDue {
            next_block
        } if next_block == not_due_block + 3
    ));

    // Quorum gating blocks too-small settlements
    let quorum_err = settle_energy_delivery(
        "buyer-1".into(),
        &provider.provider_id,
        50,
        not_due_block + 3,
        credit.meter_reading_hash,
    )
    .expect_err("quorum check");
    assert!(matches!(
        quorum_err,
        EnergyMarketError::SettlementBelowQuorum { .. }
    ));

    let slashes = slashes_page(Some(&provider.provider_id), 0, 10);
    assert!(
        slashes.items.iter().any(|slash| slash.reason == "quorum"),
        "quorum shortfall should record a slash"
    );

    // Valid settlement succeeds
    let receipt = settle_energy_delivery(
        "buyer-1".into(),
        &provider.provider_id,
        600,
        15,
        credit.meter_reading_hash,
    )
    .expect("settlement ok");
    assert_eq!(receipt.kwh_delivered, 600);
    let anchored = the_block::energy::anchored_receipts();
    assert!(
        anchored
            .iter()
            .any(|r| r.meter_reading_hash == credit.meter_reading_hash),
        "anchored receipts should persist settlement history"
    );

    let fake_hash = [9u8; 32];
    let conflict_err =
        settle_energy_delivery("buyer-1".into(), &provider.provider_id, 10, 20, fake_hash)
            .expect_err("unknown reading should fail");
    assert!(matches!(conflict_err, EnergyMarketError::UnknownReading(_)));
    let conflict_slashes = slashes_page(Some(&provider.provider_id), 0, 10);
    assert!(
        conflict_slashes
            .items
            .iter()
            .any(|slash| slash.reason == "conflict"),
        "Unknown reading should produce a conflict slash"
    );

    // Dispute open/resolve flow
    let dispute = flag_dispute(
        "reporter-1".into(),
        credit.meter_reading_hash,
        "accuracy".into(),
        22,
    )
    .expect("dispute opened");
    assert_eq!(dispute.status, DisputeStatus::Open);
    let resolved = resolve_dispute(dispute.id, "arbiter".into(), Some("resolved".into()), 25)
        .expect("dispute resolved");
    assert_eq!(resolved.status, DisputeStatus::Resolved);
}
