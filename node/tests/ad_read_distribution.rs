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
