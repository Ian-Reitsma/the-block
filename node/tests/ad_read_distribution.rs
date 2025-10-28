#![cfg(feature = "integration-tests")]

use ad_market::SettlementBreakdown;
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::OsRng;
use sys::tempfile::tempdir;
use the_block::{Blockchain, ReadAck};

fn build_signed_ack(bytes: u64, domain: &str, provider: &str) -> ReadAck {
    let mut rng = OsRng::default();
    let signing = SigningKey::generate(&mut rng);
    let verifying = signing.verifying_key();
    let manifest = [0x11; 32];
    let path_hash = [0x22; 32];
    let ts = 1_700_000_000u64;
    let mut client_hash = [0u8; 32];
    client_hash[0] = 7;

    let mut hasher = Hasher::new();
    hasher.update(&manifest);
    hasher.update(&path_hash);
    hasher.update(&bytes.to_le_bytes());
    hasher.update(&ts.to_le_bytes());
    hasher.update(&client_hash);
    let message = hasher.finalize();
    let signature = signing.sign(message.as_bytes());

    ReadAck {
        manifest,
        path_hash,
        bytes,
        ts,
        client_hash,
        pk: verifying.to_bytes(),
        sig: signature.to_bytes().to_vec(),
        domain: domain.to_string(),
        provider: provider.to_string(),
        campaign_id: Some("cmp-1".into()),
        creative_id: Some("creative-1".into()),
        readiness: None,
        zk_proof: None,
    }
}

#[test]
fn mixed_subsidy_and_ad_flows_persist_in_block_and_accounts() {
    let dir = tempdir().expect("temp dir");
    let chain_path = dir.path().join("chain");
    let mut bc = Blockchain::new(chain_path.to_str().expect("path"));
    bc.difficulty = 0;
    bc.gamma_read_sub_ct_raw = 1;
    bc.params.read_subsidy_viewer_percent = 40;
    bc.params.read_subsidy_host_percent = 20;
    bc.params.read_subsidy_hardware_percent = 10;
    bc.params.read_subsidy_verifier_percent = 10;
    bc.params.read_subsidy_liquidity_percent = 10;
    bc.recompute_difficulty();
    bc.add_account("miner".into(), 0, 0).expect("miner account");

    let ack = build_signed_ack(450, "example.com", "edge-1");
    bc.submit_read_ack(ack.clone()).expect("ack accepted");

    let settlement = SettlementBreakdown {
        campaign_id: "cmp-1".into(),
        creative_id: "creative-1".into(),
        bytes: 450,
        price_per_mib_usd_micros: 80,
        total_usd_micros: 80,
        demand_usd_micros: 80,
        total_ct: 80,
        viewer_ct: 30,
        host_ct: 20,
        hardware_ct: 10,
        verifier_ct: 5,
        liquidity_ct: 5,
        miner_ct: 10,
        host_it: 0,
        hardware_it: 0,
        verifier_it: 0,
        liquidity_it: 0,
        miner_it: 0,
        unsettled_usd_micros: 0,
        ct_price_usd_micros: 1,
        it_price_usd_micros: 1,
    };
    bc.record_ad_settlement(&ack, settlement);

    let block = bc.mine_block_at("miner", 1).expect("mined block");
    assert!(bc.pending_ad_settlements.is_empty());

    assert_eq!(block.read_sub_ct.value(), 450);
    assert_eq!(block.read_sub_viewer_ct.value(), 200);
    assert_eq!(block.read_sub_host_ct.value(), 100);
    assert_eq!(block.read_sub_hardware_ct.value(), 50);
    assert_eq!(block.read_sub_verifier_ct.value(), 50);
    assert_eq!(block.read_sub_liquidity_ct.value(), 55);
    assert_eq!(block.ad_viewer_ct.value(), 30);
    assert_eq!(block.ad_host_ct.value(), 20);
    assert_eq!(block.ad_hardware_ct.value(), 10);
    assert_eq!(block.ad_verifier_ct.value(), 5);
    assert_eq!(block.ad_liquidity_ct.value(), 5);
    assert_eq!(block.ad_miner_ct.value(), 10);

    let viewer_addr = format!("0000:{}", hex::encode(ack.pk));
    let host_addr = format!("0001:host:{}", ack.domain);
    let hardware_addr = format!("0002:hardware:{}", ack.provider);
    let verifier_addr = format!("0003:verifier:{}", ack.domain);
    let liquidity_addr = "0004:liquidity:pool";

    let viewer_balance = bc
        .get_account_balance(&viewer_addr)
        .expect("viewer balance");
    assert_eq!(
        viewer_balance.consumer,
        block.read_sub_viewer_ct.value() + block.ad_viewer_ct.value()
    );

    let host_balance = bc.get_account_balance(&host_addr).expect("host balance");
    assert_eq!(
        host_balance.consumer,
        block.read_sub_host_ct.value() + block.ad_host_ct.value()
    );

    let hardware_balance = bc
        .get_account_balance(&hardware_addr)
        .expect("hardware balance");
    assert_eq!(
        hardware_balance.consumer,
        block.read_sub_hardware_ct.value() + block.ad_hardware_ct.value()
    );

    let verifier_balance = bc
        .get_account_balance(&verifier_addr)
        .expect("verifier balance");
    assert_eq!(
        verifier_balance.consumer,
        block.read_sub_verifier_ct.value() + block.ad_verifier_ct.value()
    );

    let liquidity_balance = bc
        .get_account_balance(liquidity_addr)
        .expect("liquidity balance");
    assert_eq!(
        liquidity_balance.consumer,
        block.read_sub_liquidity_ct.value()
    );
}

#[test]
fn dual_token_liquidity_splits_roll_into_block_totals() {
    let dir = tempdir().expect("temp dir");
    let chain_path = dir.path().join("chain");
    let mut bc = Blockchain::new(chain_path.to_str().expect("path"));
    bc.difficulty = 0;
    bc.gamma_read_sub_ct_raw = 1;
    bc.params.read_subsidy_viewer_percent = 40;
    bc.params.read_subsidy_host_percent = 20;
    bc.params.read_subsidy_hardware_percent = 10;
    bc.params.read_subsidy_verifier_percent = 10;
    bc.params.read_subsidy_liquidity_percent = 10;
    bc.recompute_difficulty();
    bc.add_account("miner".into(), 0, 0).expect("miner account");
    bc.snapshot.set_interval(1);

    let ack_a = build_signed_ack(256, "example.com", "edge-a");
    let ack_b = build_signed_ack(512, "example.com", "edge-b");
    bc.submit_read_ack(ack_a.clone()).expect("ack A accepted");
    bc.submit_read_ack(ack_b.clone()).expect("ack B accepted");

    let settlement_a = SettlementBreakdown {
        campaign_id: "cmp-1".into(),
        creative_id: "creative-1".into(),
        bytes: 256,
        price_per_mib_usd_micros: 120,
        total_usd_micros: 120,
        demand_usd_micros: 160,
        total_ct: 35,
        viewer_ct: 12,
        host_ct: 8,
        hardware_ct: 6,
        verifier_ct: 4,
        liquidity_ct: 2,
        miner_ct: 3,
        host_it: 5,
        hardware_it: 7,
        verifier_it: 9,
        liquidity_it: 11,
        miner_it: 13,
        unsettled_usd_micros: 0,
        ct_price_usd_micros: 1,
        it_price_usd_micros: 1,
    };
    let settlement_b = SettlementBreakdown {
        campaign_id: "cmp-2".into(),
        creative_id: "creative-2".into(),
        bytes: 512,
        price_per_mib_usd_micros: 150,
        total_usd_micros: 150,
        demand_usd_micros: 180,
        total_ct: 30,
        viewer_ct: 7,
        host_ct: 5,
        hardware_ct: 4,
        verifier_ct: 3,
        liquidity_ct: 9,
        miner_ct: 2,
        host_it: 4,
        hardware_it: 3,
        verifier_it: 2,
        liquidity_it: 1,
        miner_it: 0,
        unsettled_usd_micros: 0,
        ct_price_usd_micros: 1,
        it_price_usd_micros: 1,
    };
    bc.record_ad_settlement(&ack_a, settlement_a.clone());
    bc.record_ad_settlement(&ack_b, settlement_b.clone());

    let pending = bc.pending_ad_settlements.clone();
    assert_eq!(pending.len(), 2);
    let mut totals = (
        0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64,
    );
    for record in pending {
        totals.0 = totals.0.saturating_add(record.viewer_ct);
        totals.1 = totals.1.saturating_add(record.host_ct);
        totals.2 = totals.2.saturating_add(record.hardware_ct);
        totals.3 = totals.3.saturating_add(record.verifier_ct);
        totals.4 = totals.4.saturating_add(record.liquidity_ct);
        totals.5 = totals.5.saturating_add(record.miner_ct);
        totals.6 = totals.6.saturating_add(record.host_it);
        totals.7 = totals.7.saturating_add(record.hardware_it);
        totals.8 = totals.8.saturating_add(record.verifier_it);
        totals.9 = totals.9.saturating_add(record.liquidity_it);
        totals.10 = totals.10.saturating_add(record.miner_it);
        totals.11 = totals.11.saturating_add(record.total_usd_micros);
        totals.12 = totals.12.saturating_add(record.total_ct);
    }

    assert_eq!(totals.0, settlement_a.viewer_ct + settlement_b.viewer_ct);
    assert_eq!(totals.1, settlement_a.host_ct + settlement_b.host_ct);
    assert_eq!(
        totals.2,
        settlement_a.hardware_ct + settlement_b.hardware_ct
    );
    assert_eq!(
        totals.3,
        settlement_a.verifier_ct + settlement_b.verifier_ct
    );
    assert_eq!(
        totals.4,
        settlement_a.liquidity_ct + settlement_b.liquidity_ct
    );
    assert_eq!(totals.5, settlement_a.miner_ct + settlement_b.miner_ct);
    assert_eq!(totals.6, settlement_a.host_it + settlement_b.host_it);
    assert_eq!(
        totals.7,
        settlement_a.hardware_it + settlement_b.hardware_it
    );
    assert_eq!(
        totals.8,
        settlement_a.verifier_it + settlement_b.verifier_it
    );
    assert_eq!(
        totals.9,
        settlement_a.liquidity_it + settlement_b.liquidity_it
    );
    assert_eq!(totals.10, settlement_a.miner_it + settlement_b.miner_it);
    assert_eq!(
        totals.11,
        settlement_a.total_usd_micros + settlement_b.total_usd_micros
    );
    assert_eq!(totals.12, settlement_a.total_ct + settlement_b.total_ct);
}

#[test]
fn dual_token_feature_flag_suppresses_it_when_disabled() {
    let dir = tempdir().expect("temp dir");
    let chain_path = dir.path().join("chain");
    let mut bc = Blockchain::new(chain_path.to_str().expect("path"));
    bc.difficulty = 0;
    bc.gamma_read_sub_ct_raw = 1;
    bc.params.dual_token_settlement_enabled = 0;
    bc.recompute_difficulty();
    bc.add_account("miner".into(), 0, 0).expect("miner account");

    let ack = build_signed_ack(128, "example.com", "edge-disabled");
    bc.submit_read_ack(ack.clone()).expect("ack accepted");

    let settlement = SettlementBreakdown {
        campaign_id: "cmp-flag".into(),
        creative_id: "creative-flag".into(),
        bytes: 128,
        price_per_mib_usd_micros: 90,
        total_usd_micros: 90,
        demand_usd_micros: 90,
        total_ct: 45,
        viewer_ct: 15,
        host_ct: 12,
        hardware_ct: 8,
        verifier_ct: 5,
        liquidity_ct: 3,
        miner_ct: 2,
        host_it: 9,
        hardware_it: 7,
        verifier_it: 5,
        liquidity_it: 4,
        miner_it: 6,
        unsettled_usd_micros: 0,
        ct_price_usd_micros: 1,
        it_price_usd_micros: 1,
    };
    bc.record_ad_settlement(&ack, settlement.clone());

    let block = bc.mine_block_at("miner", 1).expect("mined block");
    assert_eq!(block.ad_host_it.value(), 0);
    assert_eq!(block.ad_hardware_it.value(), 0);
    assert_eq!(block.ad_verifier_it.value(), 0);
    assert_eq!(block.ad_liquidity_it.value(), 0);
    assert_eq!(block.ad_miner_it.value(), 0);

    bc.params.dual_token_settlement_enabled = 1;
    bc.submit_read_ack(ack.clone())
        .expect("ack replay accepted");
    bc.record_ad_settlement(&ack, settlement);
    let enabled_block = bc.mine_block_at("miner", 2).expect("second block");
    assert_eq!(enabled_block.ad_host_it.value(), 9);
    assert_eq!(enabled_block.ad_hardware_it.value(), 7);
    assert_eq!(enabled_block.ad_verifier_it.value(), 5);
    assert_eq!(enabled_block.ad_liquidity_it.value(), 4);
    assert_eq!(enabled_block.ad_miner_it.value(), 6);
}
