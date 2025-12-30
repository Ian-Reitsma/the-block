#![cfg(feature = "integration-tests")]

use ad_market::{
    DeliveryChannel, DomainTier, ResourceFloorBreakdown, SelectionCandidateTrace,
    SelectionCohortTrace, SelectionReceipt, SettlementBreakdown, UpliftEstimate,
};
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::hex;
use crypto_suite::signatures::ed25519::SigningKey;
use rand::rngs::OsRng;
use sys::tempfile::tempdir;
use the_block::{Blockchain, ReadAck, Receipt};

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
        selection_receipt: None,
        geo: None,
        device: None,
        crm_lists: Vec::new(),
        delivery_channel: DeliveryChannel::Http,
        mesh: None,
        badge_soft_intent: None,
        readiness: None,
        zk_proof: None,
        presence_badge: None,
        venue_id: None,
        crowd_size_hint: None,
    }
}

fn dummy_receipt(
    campaign_id: &str,
    creative_id: &str,
    clearing_price: u64,
    resource_floor: u64,
    runner_up: u64,
    quality_bid: u64,
) -> SelectionReceipt {
    SelectionReceipt {
        cohort: SelectionCohortTrace {
            domain: "example.com".into(),
            domain_tier: DomainTier::default(),
            domain_owner: None,
            provider: Some("provider".into()),
            badges: Vec::new(),
            interest_tags: Vec::new(),
            presence_bucket: None,
            selectors_version: 0,
            bytes: 0,
            price_per_mib_usd_micros: 0,
            delivery_channel: DeliveryChannel::Http,
            mesh_peer: None,
            mesh_transport: None,
            mesh_latency_ms: None,
        },
        candidates: vec![SelectionCandidateTrace {
            campaign_id: campaign_id.into(),
            creative_id: creative_id.into(),
            base_bid_usd_micros: quality_bid,
            quality_adjusted_bid_usd_micros: quality_bid,
            available_budget_usd_micros: quality_bid,
            action_rate_ppm: 0,
            lift_ppm: 0,
            quality_multiplier: 1.0,
            pacing_kappa: 1.0,
            requested_kappa: 1.0,
            shading_multiplier: 1.0,
            predicted_lift_ppm: 0,
            baseline_action_rate_ppm: 0,
            predicted_propensity: 0.0,
            uplift_sample_size: 0,
            uplift_ece: 0.0,
            shadow_price: 0.0,
            dual_price: 0.0,
            delivery_channel: DeliveryChannel::Http,
            preferred_delivery_match: false,
        }],
        winner_index: 0,
        resource_floor_usd_micros: resource_floor,
        resource_floor_breakdown: floor_breakdown(resource_floor),
        runner_up_quality_bid_usd_micros: runner_up,
        clearing_price_usd_micros: clearing_price,
        attestation: None,
        proof_metadata: None,
        verifier_committee: None,
        verifier_stake_snapshot: None,
        verifier_transcript: Vec::new(),
        badge_soft_intent: None,
        badge_soft_intent_snapshot: None,
        uplift_assignment: None,
    }
}

fn floor_breakdown(total: u64) -> ResourceFloorBreakdown {
    ResourceFloorBreakdown {
        bandwidth_usd_micros: total,
        verifier_usd_micros: 0,
        host_usd_micros: 0,
        qualified_impressions_per_proof: 1,
    }
}

#[testkit::tb_serial]
fn mixed_subsidy_and_ad_flows_persist_in_block_and_accounts() {
    // Reset any environment variables that might affect ReadAck processing
    std::env::remove_var("TB_GATEWAY_RECEIPTS");
    std::env::remove_var("TB_DNS_DB_PATH");

    let dir = tempdir().expect("temp dir");
    let chain_path = dir.path().join("chain");
    let mut bc = Blockchain::new(chain_path.to_str().expect("path"));
    bc.difficulty = 0;
    bc.gamma_read_sub_raw = 1;
    bc.params.read_subsidy_viewer_percent = 40;
    bc.params.read_subsidy_host_percent = 20;
    bc.params.read_subsidy_hardware_percent = 10;
    bc.params.read_subsidy_verifier_percent = 10;
    bc.params.read_subsidy_liquidity_percent = 10;
    bc.recompute_difficulty();
    bc.add_account("miner".into(), 0).expect("miner account");

    let ack = build_signed_ack(450, "example.com", "edge-1");
    bc.submit_read_ack(ack.clone()).expect("ack accepted");

    let settlement = SettlementBreakdown {
        campaign_id: "cmp-1".into(),
        creative_id: "creative-1".into(),
        bytes: 450,
        price_per_mib_usd_micros: 80,
        total_usd_micros: 80,
        demand_usd_micros: 80,
        resource_floor_usd_micros: 80,
        clearing_price_usd_micros: 80,
        delivery_channel: DeliveryChannel::Http,
        mesh_payload: None,
        mesh_payload_digest: None,
        resource_floor_breakdown: floor_breakdown(80),
        runner_up_quality_bid_usd_micros: 0,
        quality_adjusted_bid_usd_micros: 80,
        total: 80,
        viewer: 30,
        host: 20,
        hardware: 10,
        verifier: 5,
        liquidity: 5,
        miner: 10,
        unsettled_usd_micros: 0,
        price_usd_micros: 1,
        remainders_usd_micros: Default::default(),
        twap_window_id: 0,
        selection_receipt: dummy_receipt("cmp-1", "creative-1", 80, 80, 0, 80),
        uplift: UpliftEstimate::default(),
    };
    bc.record_ad_settlement(&ack, settlement);

    let block = bc.mine_block_at("miner", 1).expect("mined block");
    assert!(bc.pending_ad_settlements.is_empty());

    assert_eq!(block.read_sub.value(), 450);
    assert_eq!(block.read_sub_viewer.value(), 200);
    assert_eq!(block.read_sub_host.value(), 100);
    assert_eq!(block.read_sub_hardware.value(), 50);
    assert_eq!(block.read_sub_verifier.value(), 50);
    assert_eq!(block.read_sub_liquidity.value(), 55);
    assert_eq!(block.ad_viewer.value(), 30);
    assert_eq!(block.ad_host.value(), 20);
    assert_eq!(block.ad_hardware.value(), 10);
    assert_eq!(block.ad_verifier.value(), 5);
    assert_eq!(block.ad_liquidity.value(), 5);
    assert_eq!(block.ad_miner.value(), 10);

    let viewer_addr = format!("0000:{}", hex::encode(ack.pk));
    let host_addr = format!("0001:host:{}", ack.domain);
    let hardware_addr = format!("0002:hardware:{}", ack.provider);
    let verifier_addr = format!("0003:verifier:{}", ack.domain);
    let liquidity_addr = "0004:liquidity:pool";

    let viewer_balance = bc
        .get_account_balance(&viewer_addr)
        .expect("viewer balance");
    assert_eq!(
        viewer_balance.amount,
        block.read_sub_viewer.value() + block.ad_viewer.value()
    );

    let host_balance = bc.get_account_balance(&host_addr).expect("host balance");
    assert_eq!(
        host_balance.amount,
        block.read_sub_host.value() + block.ad_host.value()
    );

    let hardware_balance = bc
        .get_account_balance(&hardware_addr)
        .expect("hardware balance");
    assert_eq!(
        hardware_balance.amount,
        block.read_sub_hardware.value() + block.ad_hardware.value()
    );

    let verifier_balance = bc
        .get_account_balance(&verifier_addr)
        .expect("verifier balance");
    assert_eq!(
        verifier_balance.amount,
        block.read_sub_verifier.value() + block.ad_verifier.value()
    );

    let liquidity_balance = bc
        .get_account_balance(liquidity_addr)
        .expect("liquidity balance");
    assert_eq!(liquidity_balance.amount, block.read_sub_liquidity.value());

    assert_eq!(block.receipts.len(), 1);
    match &block.receipts[0] {
        Receipt::Ad(ad) => {
            assert_eq!(ad.campaign_id, "cmp-1");
            assert_eq!(ad.publisher, host_addr);
            assert_eq!(ad.impressions, 1);
            assert_eq!(ad.spend, 80);
            assert_eq!(ad.block_height, block.index);
            assert_eq!(ad.conversions, 0);
        }
        other => panic!("expected ad receipt, got {:?}", other.market_name()),
    }
}
