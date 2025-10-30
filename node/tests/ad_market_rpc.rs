#![cfg(feature = "integration-tests")]

use std::collections::{HashMap, HashSet};
use std::fs;
#[cfg(feature = "python-bindings")]
use std::io::{Read, Write};
#[cfg(feature = "python-bindings")]
use std::net::TcpStream;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sys::tempfile::TempDir;

use ad_market::{
    Campaign, CampaignTargeting, Creative, CreativePlacement, DeliveryChannel, DistributionPolicy,
    ImpressionContext, InMemoryMarketplace, Marketplace, MarketplaceConfig, MarketplaceHandle,
    MeshContext, ReservationKey, SelectionAttestation, SelectionAttestationKind, SelectionReceipt,
    SledMarketplace, VerifierCommitteeConfig, MICROS_PER_DOLLAR,
};
use crypto_suite::{encoding::hex, hashing::blake3, vrf};
use foundation_rpc::{Request as RpcRequest, Response, RpcError};
use foundation_serialization::json::{self as json_mod, Value};
use rand::rngs::StdRng;
#[cfg(feature = "python-bindings")]
use the_block::serve_metrics_with_shutdown;
use the_block::{
    ad_readiness::{AdReadinessConfig, AdReadinessHandle},
    identity::{handle_registry::HandleRegistry, DidRegistry},
    rpc::{fuzz_dispatch_request, fuzz_runtime_config_with_admin, RpcRuntimeConfig},
    Blockchain,
};
use verifier_selection::{self, CommitteeConfig as CommitteePolicy, StakeEntry, StakeSnapshot};
use zkp::selection::{self, SelectionProofPublicInputs};

mod util;

fn parse_json(src: &str) -> Value {
    json_mod::from_str(src).expect("json value")
}

#[derive(Clone)]
struct RpcHarness {
    bc: Arc<Mutex<Blockchain>>,
    mining: Arc<AtomicBool>,
    nonces: Arc<Mutex<HashSet<(String, u64)>>>,
    handles: Arc<Mutex<HandleRegistry>>,
    dids: Arc<Mutex<DidRegistry>>,
    runtime_cfg: Arc<RpcRuntimeConfig>,
    market: MarketplaceHandle,
    in_memory_market: Option<Arc<InMemoryMarketplace>>,
    admin_token: String,
    readiness: Option<AdReadinessHandle>,
}

impl RpcHarness {
    fn call(&self, method: &str, params: Value) -> Response {
        self.call_with_auth(method, params, None)
    }

    fn call_with_auth(&self, method: &str, params: Value, auth: Option<String>) -> Response {
        let request = RpcRequest::new(method.to_string(), params);
        let auth_header = auth.unwrap_or_else(|| format!("Bearer {}", self.admin_token));
        fuzz_dispatch_request(
            Arc::clone(&self.bc),
            Arc::clone(&self.mining),
            Arc::clone(&self.nonces),
            Arc::clone(&self.handles),
            Arc::clone(&self.dids),
            Arc::clone(&self.runtime_cfg),
            Some(Arc::clone(&self.market)),
            self.readiness.clone(),
            request,
            Some(auth_header),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        )
    }
}

fn expect_ok(response: Response) -> Value {
    match response {
        Response::Result { result, .. } => result,
        Response::Error { error, .. } => panic!("rpc error: {} ({})", error.message(), error.code),
    }
}

fn expect_error(response: Response) -> RpcError {
    match response {
        Response::Error { error, .. } => error,
        Response::Result { .. } => panic!("expected rpc error"),
    }
}

fn build_in_memory_harness(
    name: &str,
    config: MarketplaceConfig,
) -> (TempDir, Arc<RpcHarness>, AdReadinessHandle) {
    let dir = util::temp::temp_dir(name);
    let chain_path = dir.path().join("chain");
    fs::create_dir_all(&chain_path).expect("chain path");
    let bc = Arc::new(Mutex::new(Blockchain::new(
        chain_path.to_str().expect("chain path"),
    )));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::new()));

    let handles_path = dir.path().join("handles");
    fs::create_dir_all(&handles_path).expect("handles path");
    let handles = Arc::new(Mutex::new(HandleRegistry::open(
        handles_path.to_str().expect("handles path"),
    )));

    let dids_path = dir.path().join("dids");
    fs::create_dir_all(&dids_path).expect("dids path");
    let dids = Arc::new(Mutex::new(DidRegistry::open(&dids_path)));

    let admin_token = format!("integration-token-{}", name);
    let runtime_cfg = fuzz_runtime_config_with_admin(admin_token.clone());

    let market_impl = Arc::new(InMemoryMarketplace::new(config));
    let market: MarketplaceHandle = market_impl.clone();
    let readiness = AdReadinessHandle::new(AdReadinessConfig {
        window_secs: 300,
        min_unique_viewers: 1,
        min_host_count: 1,
        min_provider_count: 1,
    });

    let harness = Arc::new(RpcHarness {
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg,
        market,
        in_memory_market: Some(market_impl),
        admin_token,
        readiness: Some(readiness.clone()),
    });

    (dir, harness, readiness)
}

const SELECTION_CIRCUIT_ID: &str = "selection_argmax_v1";
const SELECTION_CIRCUIT_REVISION: u16 = 1;

fn encode_bytes(bytes: &[u8]) -> String {
    let body = bytes
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

fn build_snark_proof(receipt: &SelectionReceipt) -> Vec<u8> {
    let commitment = receipt
        .commitment_digest()
        .expect("commitment digest computed");
    let winner = &receipt.candidates[receipt.winner_index];
    let inputs = SelectionProofPublicInputs {
        commitment: commitment.to_vec(),
        winner_index: receipt.winner_index as u16,
        winner_quality_bid_usd_micros: winner.quality_adjusted_bid_usd_micros,
        runner_up_quality_bid_usd_micros: receipt.runner_up_quality_bid_usd_micros,
        resource_floor_usd_micros: receipt.resource_floor_usd_micros,
        clearing_price_usd_micros: receipt.clearing_price_usd_micros,
        candidate_count: receipt.candidates.len() as u16,
    };
    let proof_bytes = vec![0xAB; 160];
    let proof_bytes_digest = selection::proof_bytes_digest(&proof_bytes);
    let transcript = selection::expected_transcript_digest(
        SELECTION_CIRCUIT_ID,
        SELECTION_CIRCUIT_REVISION,
        &proof_bytes_digest,
        &inputs,
    )
    .expect("transcript digest");
    let commitments_json = format!(
        "[{},{}]",
        encode_bytes(&[0x44u8; 32]),
        encode_bytes(&[0x77u8; 32])
    );
    let public_inputs_json = format!(
        "{{\"commitment\":{},\"winner_index\":{},\"winner_quality_bid_usd_micros\":{},\"runner_up_quality_bid_usd_micros\":{},\"resource_floor_usd_micros\":{},\"clearing_price_usd_micros\":{},\"candidate_count\":{}}}",
        encode_bytes(&inputs.commitment),
        inputs.winner_index,
        inputs.winner_quality_bid_usd_micros,
        inputs.runner_up_quality_bid_usd_micros,
        inputs.resource_floor_usd_micros,
        inputs.clearing_price_usd_micros,
        inputs.candidate_count,
    );
    let proof_json = format!(
        "{{\"version\":1,\"circuit_revision\":{},\"public_inputs\":{},\"proof\":{{\"protocol\":\"groth16\",\"transcript_digest\":{},\"bytes\":{},\"witness_commitments\":{}}}}}",
        SELECTION_CIRCUIT_REVISION,
        public_inputs_json,
        encode_bytes(&transcript),
        encode_bytes(&proof_bytes),
        commitments_json,
    );
    proof_json.into_bytes()
}

struct CommitteeFixture {
    policy: CommitteePolicy,
    snapshot: StakeSnapshot,
    transcript: Vec<u8>,
    receipt: verifier_selection::SelectionReceipt,
    public_key: vrf::PublicKey,
}

fn committee_fixture() -> CommitteeFixture {
    let snapshot = StakeSnapshot {
        staking_epoch: 77,
        verifiers: vec![
            StakeEntry {
                verifier_id: "alpha".into(),
                stake_units: 1_000,
            },
            StakeEntry {
                verifier_id: "beta".into(),
                stake_units: 2_000,
            },
            StakeEntry {
                verifier_id: "gamma".into(),
                stake_units: 4_000,
            },
        ],
    };
    let policy = CommitteePolicy {
        label: "verifier-selection".into(),
        committee_size: 2,
        minimum_total_stake: 1,
        stake_threshold_ppm: 0,
    };
    let transcript = b"committee-fixture".to_vec();
    let mut rng = StdRng::seed_from_u64(41);
    let (secret, public) = vrf::SecretKey::generate(&mut rng);
    let selection = verifier_selection::select_committee(&secret, &policy, &snapshot, &transcript)
        .expect("committee selected");
    CommitteeFixture {
        policy,
        snapshot,
        transcript,
        receipt: selection.receipt,
        public_key: public,
    }
}

fn verifier_committee_config(fixture: &CommitteeFixture) -> VerifierCommitteeConfig {
    VerifierCommitteeConfig {
        vrf_public_key_hex: hex::encode(fixture.public_key.to_bytes()),
        committee_size: fixture.policy.committee_size,
        minimum_total_stake: fixture.policy.minimum_total_stake,
        stake_threshold_ppm: fixture.policy.stake_threshold_ppm,
        label: fixture.policy.label.clone(),
        require_snapshot: true,
    }
}

fn make_reservation_key(seed: u64) -> ReservationKey {
    let mut manifest = [0u8; 32];
    manifest[..8].copy_from_slice(&seed.to_le_bytes());
    let mut path_hash = [0u8; 32];
    path_hash[8..16].copy_from_slice(&seed.to_le_bytes());
    let mut discriminator = [0u8; 32];
    discriminator[16..24].copy_from_slice(&seed.to_le_bytes());
    ReservationKey {
        manifest,
        path_hash,
        discriminator,
    }
}

#[testkit::tb_serial]
fn ad_market_rpc_endpoints_round_trip() {
    let dir = util::temp::temp_dir("ad_market_rpc");
    let chain_path = dir.path().join("chain");
    fs::create_dir_all(&chain_path).expect("chain path");
    let bc = Arc::new(Mutex::new(Blockchain::new(
        chain_path.to_str().expect("chain path"),
    )));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::new()));

    let handles_path = dir.path().join("handles");
    fs::create_dir_all(&handles_path).expect("handles path");
    let handles = Arc::new(Mutex::new(HandleRegistry::open(
        handles_path.to_str().expect("handles path"),
    )));

    let dids_path = dir.path().join("dids");
    fs::create_dir_all(&dids_path).expect("dids path");
    let dids = Arc::new(Mutex::new(DidRegistry::open(&dids_path)));

    let admin_token = "integration-token".to_string();
    let runtime_cfg = fuzz_runtime_config_with_admin(admin_token.clone());

    let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
    let market_dir = dir.path().join("market");
    let sled = SledMarketplace::open(
        &market_dir,
        MarketplaceConfig {
            distribution,
            ..MarketplaceConfig::default()
        },
    )
    .expect("market opened");
    let market: MarketplaceHandle = Arc::new(sled);
    let readiness = AdReadinessHandle::new(AdReadinessConfig {
        window_secs: 300,
        min_unique_viewers: 1,
        min_host_count: 1,
        min_provider_count: 1,
    });

    let harness = Arc::new(RpcHarness {
        bc,
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg,
        market,
        in_memory_market: None,
        admin_token,
        readiness: Some(readiness.clone()),
    });

    let campaign = parse_json(
        r#"{
            "id": "cmp-1",
            "advertiser_account": "adv-1",
            "budget_usd_micros": 5000000,
            "creatives": [
                {
                    "id": "creative-1",
                    "action_rate_ppm": 500000,
                    "margin_ppm": 800000,
                    "value_per_action_usd_micros": 1000000,
                    "max_cpi_usd_micros": 1500000,
                    "badges": ["physical_presence"],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": ["physical_presence"]
            },
            "metadata": {}
        }"#,
    );

    let register = harness.call("ad_market.register_campaign", campaign.clone());
    let register_value = expect_ok(register);
    assert_eq!(register_value["status"].as_str(), Some("ok"));

    let inventory = expect_ok(harness.call("ad_market.inventory", Value::Null));
    assert_eq!(inventory["status"].as_str(), Some("ok"));
    let campaigns = inventory["campaigns"].as_array().expect("campaigns array");
    assert_eq!(campaigns.len(), 1);
    let entry = &campaigns[0];
    assert_eq!(entry["id"].as_str(), Some("cmp-1"));
    assert_eq!(entry["advertiser_account"].as_str(), Some("adv-1"));
    assert_eq!(
        entry["remaining_budget_usd_micros"].as_u64(),
        Some(5_000_000)
    );
    let creative_ids = entry["creatives"].as_array().expect("creatives array");
    assert_eq!(creative_ids.len(), 1);
    assert_eq!(creative_ids[0].as_str(), Some("creative-1"));
    assert_eq!(
        inventory["oracle"]["ct_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    assert_eq!(
        inventory["oracle"]["it_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    let cohorts = inventory["cohort_prices"]
        .as_array()
        .expect("cohort prices");
    if let Some(first) = cohorts.get(0).and_then(Value::as_object) {
        assert_eq!(first["observed_utilization_ppm"].as_u64(), Some(0));
    }

    let budget_value = expect_ok(harness.call("ad_market.budget", Value::Null));
    assert_eq!(budget_value["status"].as_str(), Some("ok"));
    assert!(budget_value["config"].is_object());
    assert!(budget_value["campaigns"].is_array());

    let distribution_resp = expect_ok(harness.call("ad_market.distribution", Value::Null));
    assert_eq!(distribution_resp["status"].as_str(), Some("ok"));
    let dist = &distribution_resp["distribution"];
    assert_eq!(dist["viewer_percent"].as_u64(), Some(40));
    assert_eq!(dist["host_percent"].as_u64(), Some(30));
    assert_eq!(dist["hardware_percent"].as_u64(), Some(20));
    assert_eq!(dist["verifier_percent"].as_u64(), Some(5));
    assert_eq!(dist["liquidity_percent"].as_u64(), Some(5));
    assert_eq!(
        dist["liquidity_split_ct_ppm"].as_u64(),
        Some(DistributionPolicy::default().liquidity_split_ct_ppm as u64)
    );

    let readiness_initial = expect_ok(harness.call("ad_market.readiness", Value::Null));
    assert_eq!(readiness_initial["status"].as_str(), Some("ok"));
    assert_eq!(readiness_initial["ready"].as_bool(), Some(false));
    let blockers = readiness_initial["blockers"]
        .as_array()
        .expect("blockers array");
    assert!(blockers
        .iter()
        .any(|value| value.as_str() == Some("insufficient_unique_viewers")));
    assert_eq!(
        readiness_initial["distribution"]["viewer_percent"].as_u64(),
        Some(40)
    );
    let utilization_initial = readiness_initial
        .get("utilization")
        .and_then(Value::as_object)
        .expect("utilization map");
    let initial_cohort_count = utilization_initial["cohort_count"].as_u64().unwrap_or(0);
    assert!(initial_cohort_count <= 1);
    assert_eq!(utilization_initial["mean_ppm"].as_u64(), Some(0));
    assert_eq!(utilization_initial["max_ppm"].as_u64(), Some(0));
    let util_cohorts = utilization_initial["cohorts"]
        .as_array()
        .expect("cohort util");
    if let Some(util_entry) = util_cohorts.get(0).and_then(Value::as_object) {
        assert_eq!(util_entry["domain"].as_str(), Some("example.test"));
        assert_eq!(util_entry["observed_utilization_ppm"].as_u64(), Some(0));
        assert_eq!(util_entry["delta_utilization_ppm"].as_i64(), Some(0));
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("timestamp")
        .as_secs();
    readiness.record_ack(now, [0u8; 32], "example.test", Some("provider-ready"));
    let readiness_ready = expect_ok(harness.call("ad_market.readiness", Value::Null));
    assert_eq!(readiness_ready["status"].as_str(), Some("ok"));
    assert_eq!(readiness_ready["ready"].as_bool(), Some(true));
    assert_eq!(readiness_ready["unique_viewers"].as_u64(), Some(1));
    assert_eq!(
        readiness_ready["blockers"]
            .as_array()
            .expect("blockers array")
            .len(),
        0
    );
    assert!(readiness_ready["ct_price_usd_micros"].as_u64().is_some());
    assert!(readiness_ready["it_price_usd_micros"].as_u64().is_some());
    let oracle = readiness_ready["oracle"].as_object().expect("oracle map");
    let snapshot_oracle = oracle["snapshot"].as_object().expect("snapshot oracle");
    assert!(snapshot_oracle["ct_price_usd_micros"].as_u64().is_some());
    assert!(snapshot_oracle["it_price_usd_micros"].as_u64().is_some());
    let market_oracle = oracle["market"].as_object().expect("market oracle");
    assert!(market_oracle["ct_price_usd_micros"].as_u64().is_some());
    assert!(market_oracle["it_price_usd_micros"].as_u64().is_some());
    let utilization_ready = readiness_ready
        .get("utilization")
        .and_then(Value::as_object)
        .expect("utilization map");
    let ready_cohort_count = utilization_ready["cohort_count"].as_u64().unwrap_or(0);
    assert!(ready_cohort_count <= 1);
    let ready_cohorts = utilization_ready["cohorts"]
        .as_array()
        .expect("ready cohorts");
    if let Some(ready_entry) = ready_cohorts.get(0).and_then(Value::as_object) {
        assert_eq!(ready_entry["observed_utilization_ppm"].as_u64(), Some(0));
        assert_eq!(ready_entry["delta_utilization_ppm"].as_i64(), Some(0));
    }

    let duplicate = expect_error(harness.call(
        "ad_market.register_campaign",
        parse_json(
            r#"{
                "id": "cmp-1",
                "advertiser_account": "adv-1",
                "budget_usd_micros": 1000000,
                "creatives": [
                    {
                        "id": "creative-dup",
                        "action_rate_ppm": 300000,
                        "margin_ppm": 700000,
                        "value_per_action_usd_micros": 500000,
                        "max_cpi_usd_micros": 600000,
                        "badges": [],
                        "domains": ["example.test"],
                        "metadata": {}
                    }
                ],
                "targeting": {
                    "domains": ["example.test"],
                    "badges": []
                },
                "metadata": {}
            }"#,
        ),
    ));
    assert_eq!(duplicate.code, -32000);
    assert_eq!(duplicate.message(), "campaign already exists");

    let invalid = expect_error(harness.call(
        "ad_market.register_campaign",
        parse_json(r#"{ "id": "cmp-invalid" }"#),
    ));
    assert_eq!(invalid.code, -32602);
    assert_eq!(invalid.message(), "invalid params");

    harness
        .market
        .update_distribution(DistributionPolicy::new(45, 35, 10, 5, 5));
    let updated = expect_ok(harness.call("ad_market.distribution", Value::Null));
    let updated_dist = &updated["distribution"];
    assert_eq!(updated_dist["viewer_percent"].as_u64(), Some(45));
    assert_eq!(updated_dist["host_percent"].as_u64(), Some(35));
    assert_eq!(updated_dist["hardware_percent"].as_u64(), Some(10));

    let concurrent_payload = parse_json(
        r#"{
            "id": "cmp-concurrent",
            "advertiser_account": "adv-2",
            "budget_usd_micros": 2500000,
            "creatives": [
                {
                    "id": "creative-concurrent",
                    "action_rate_ppm": 600000,
                    "margin_ppm": 750000,
                    "value_per_action_usd_micros": 800000,
                    "max_cpi_usd_micros": 900000,
                    "badges": [],
                    "domains": ["concurrent.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["concurrent.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );

    let harness_for_threads = Arc::clone(&harness);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let harness = Arc::clone(&harness_for_threads);
            let payload = concurrent_payload.clone();
            std::thread::spawn(move || harness.call("ad_market.register_campaign", payload))
        })
        .collect();

    let mut ok_count = 0;
    let mut duplicate_count = 0;
    for handle in handles {
        let response = handle.join().expect("thread join");
        match response {
            Response::Result { .. } => ok_count += 1,
            Response::Error { error, .. } => {
                if error.message() == "campaign already exists" {
                    duplicate_count += 1;
                } else {
                    panic!("unexpected error: {}", error.message());
                }
            }
        }
    }
    assert_eq!(ok_count, 1);
    assert_eq!(duplicate_count, 1);

    let post_inventory = expect_ok(harness.call("ad_market.inventory", Value::Null));
    let campaigns = post_inventory["campaigns"]
        .as_array()
        .expect("campaigns array");
    assert_eq!(campaigns.len(), 2);
}

#[testkit::tb_serial]
fn governance_updates_distribution_policy() {
    let dir = util::temp::temp_dir("ad_market_gov_sync");
    let chain_path = dir.path().join("chain");
    fs::create_dir_all(&chain_path).expect("chain path");
    let bc = Arc::new(Mutex::new(Blockchain::new(
        chain_path.to_str().expect("chain path"),
    )));
    let mining = Arc::new(AtomicBool::new(false));
    let nonces = Arc::new(Mutex::new(HashSet::new()));
    let handles_path = dir.path().join("handles");
    fs::create_dir_all(&handles_path).expect("handles path");
    let handles = Arc::new(Mutex::new(HandleRegistry::open(
        handles_path.to_str().expect("handles path"),
    )));
    let dids_path = dir.path().join("dids");
    fs::create_dir_all(&dids_path).expect("dids path");
    let dids = Arc::new(Mutex::new(DidRegistry::open(&dids_path)));
    let admin_token = "integration-token".to_string();
    let runtime_cfg = fuzz_runtime_config_with_admin(admin_token.clone());
    let distribution = DistributionPolicy::new(40, 30, 20, 5, 5);
    let market_dir = dir.path().join("market");
    let sled = SledMarketplace::open(
        &market_dir,
        MarketplaceConfig {
            distribution,
            ..MarketplaceConfig::default()
        },
    )
    .expect("market opened");
    let market: MarketplaceHandle = Arc::new(sled);

    let harness = RpcHarness {
        bc: Arc::clone(&bc),
        mining,
        nonces,
        handles,
        dids,
        runtime_cfg,
        market: market.clone(),
        admin_token,
        readiness: None,
    };

    {
        let mut chain = bc.lock().unwrap();
        let mut params = chain.params.clone();
        let mut runtime = the_block::governance::Runtime::with_market(&mut *chain, market.clone());
        runtime.set_current_params(&params);

        let specs = the_block::governance::registry();
        let viewer = specs
            .iter()
            .find(|spec| spec.key == the_block::governance::ParamKey::ReadSubsidyViewerPercent)
            .unwrap();
        (viewer.apply)(55, &mut params).unwrap();
        (viewer.apply_runtime)(55, &mut runtime).unwrap();

        let host = specs
            .iter()
            .find(|spec| spec.key == the_block::governance::ParamKey::ReadSubsidyHostPercent)
            .unwrap();
        (host.apply)(25, &mut params).unwrap();
        (host.apply_runtime)(25, &mut runtime).unwrap();

        let hardware = specs
            .iter()
            .find(|spec| spec.key == the_block::governance::ParamKey::ReadSubsidyHardwarePercent)
            .unwrap();
        (hardware.apply)(12, &mut params).unwrap();
        (hardware.apply_runtime)(12, &mut runtime).unwrap();

        let verifier = specs
            .iter()
            .find(|spec| spec.key == the_block::governance::ParamKey::ReadSubsidyVerifierPercent)
            .unwrap();
        (verifier.apply)(5, &mut params).unwrap();
        (verifier.apply_runtime)(5, &mut runtime).unwrap();

        let liquidity = specs
            .iter()
            .find(|spec| spec.key == the_block::governance::ParamKey::ReadSubsidyLiquidityPercent)
            .unwrap();
        (liquidity.apply)(3, &mut params).unwrap();
        (liquidity.apply_runtime)(3, &mut runtime).unwrap();

        runtime.clear_current_params();
        chain.params = params;
    }

    let response = harness.call("ad_market.distribution", Value::Null);
    let value = expect_ok(response);
    let dist = &value["distribution"];
    assert_eq!(dist["viewer_percent"].as_u64(), Some(55));
    assert_eq!(dist["host_percent"].as_u64(), Some(25));
    assert_eq!(dist["hardware_percent"].as_u64(), Some(12));
    assert_eq!(dist["verifier_percent"].as_u64(), Some(5));
    assert_eq!(dist["liquidity_percent"].as_u64(), Some(3));
}

#[testkit::tb_serial]
fn ad_market_attestation_budget_load() {
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_attestation_load", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-snark",
            "advertiser_account": "adv-proof",
            "budget_usd_micros": 6000000,
            "creatives": [
                {
                    "id": "creative-proof",
                    "action_rate_ppm": 400000,
                    "margin_ppm": 700000,
                    "value_per_action_usd_micros": 800000,
                    "max_cpi_usd_micros": 1500000,
                    "lift_ppm": 450000,
                    "badges": ["badge-a", "badge-b"],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": ["badge-a", "badge-b"]
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);

    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.badges = vec!["badge-a".into(), "badge-b".into()];
    base_ctx.bytes = 512;
    base_ctx.population_estimate = Some(50);

    let iterations = 18u64;
    for iteration in 0..iterations {
        let key_probe = make_reservation_key(iteration * 2);
        let outcome_probe = market
            .reserve_impression(key_probe.clone(), base_ctx.clone())
            .expect("probe reservation");
        let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
        market.cancel(&key_probe);

        let attestation = SelectionAttestation::Snark {
            proof: proof_bytes,
            circuit_id: SELECTION_CIRCUIT_ID.into(),
        };
        let mut ctx_attested = base_ctx.clone();
        ctx_attested.attestations = vec![attestation];

        let key_main = make_reservation_key(iteration * 2 + 1);
        let outcome = market
            .reserve_impression(key_main.clone(), ctx_attested)
            .expect("attested reservation");
        let receipt = outcome.selection_receipt.clone();
        assert_eq!(receipt.attestation_kind(), SelectionAttestationKind::Snark);
        assert!(receipt.proof_metadata.is_some());
        receipt.validate().expect("receipt validates");
        let settlement = market.commit(&key_main).expect("reservation committed");
        assert_eq!(
            settlement.selection_receipt.attestation_kind(),
            SelectionAttestationKind::Snark
        );
        assert!(settlement.total_usd_micros > 0);
    }

    let budget_value = expect_ok(harness.call("ad_market.budget", Value::Null));
    assert_eq!(budget_value["status"].as_str(), Some("ok"));
    let config_value = budget_value["config"].as_object().expect("config map");
    assert!(config_value["step_size"].as_f64().is_some());
    let campaigns = budget_value["campaigns"]
        .as_array()
        .expect("campaigns array");
    let tracked = campaigns
        .iter()
        .find(|entry| entry["campaign_id"].as_str() == Some("cmp-snark"))
        .expect("campaign present");
    let remaining = tracked["remaining_budget"]
        .as_u64()
        .expect("remaining budget");
    let total = tracked["total_budget"].as_u64().expect("total budget");
    assert!(remaining < total);
    let cohorts = tracked["cohorts"].as_array().expect("cohort array");
    assert!(!cohorts.is_empty());
    let first = cohorts[0].as_object().expect("cohort entry");
    assert!(first["kappa"].as_f64().is_some());
    assert!(first["realized_spend"].as_f64().unwrap() >= 0.0);
}

#[testkit::tb_serial]
fn ad_market_broker_state_rpc_load() {
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_broker_rpc", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-rpc",
            "advertiser_account": "adv-rpc",
            "budget_usd_micros": 4200000,
            "creatives": [
                {
                    "id": "creative-rpc",
                    "action_rate_ppm": 480000,
                    "margin_ppm": 750000,
                    "value_per_action_usd_micros": 900000,
                    "max_cpi_usd_micros": 1400000,
                    "lift_ppm": 500000,
                    "badges": ["badge-a", "badge-b"],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": ["badge-a", "badge-b"]
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.badges = vec!["badge-a".into(), "badge-b".into()];
    base_ctx.bytes = 1_024;
    base_ctx.population_estimate = Some(80);

    let iterations = 20u64;
    let mut latencies = Vec::with_capacity(iterations as usize);
    let mut generated = Vec::with_capacity(iterations as usize);
    for iteration in 0..iterations {
        let key_probe = make_reservation_key(iteration * 3);
        let outcome_probe = market
            .reserve_impression(key_probe.clone(), base_ctx.clone())
            .expect("probe reservation");
        let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
        market.cancel(&key_probe);

        let attestation = SelectionAttestation::Snark {
            proof: proof_bytes,
            circuit_id: SELECTION_CIRCUIT_ID.into(),
        };
        let mut ctx_attested = base_ctx.clone();
        ctx_attested.attestations = vec![attestation];

        let key_main = make_reservation_key(iteration * 3 + 1);
        let started = Instant::now();
        let outcome = market
            .reserve_impression(key_main.clone(), ctx_attested)
            .expect("attested reservation");
        let latency = started.elapsed().as_micros();
        latencies.push(latency);
        let receipt = outcome.selection_receipt.clone();
        assert_eq!(receipt.attestation_kind(), SelectionAttestationKind::Snark);
        receipt.validate().expect("receipt validates");
        market.commit(&key_main).expect("reservation committed");

        let broker_state = expect_ok(harness.call("ad_market.broker_state", Value::Null));
        assert_eq!(broker_state["status"].as_str(), Some("ok"));
        generated.push(
            broker_state["generated_at_micros"]
                .as_u64()
                .expect("generated at"),
        );
        let summary = broker_state["summary"].as_object().expect("summary map");
        let mean_kappa = summary["mean_kappa"].as_f64().expect("mean kappa");
        assert!(
            mean_kappa <= config.budget_broker.max_kappa + f64::EPSILON,
            "mean_kappa={mean_kappa} exceeds max_kappa={}",
            config.budget_broker.max_kappa
        );
        let realized = summary["realized_spend_total"]
            .as_f64()
            .expect("realized spend");
        assert!(realized >= 0.0);
        let pacing = broker_state["pacing"].as_object().expect("pacing map");
        assert_eq!(
            pacing["campaign_count"].as_u64(),
            Some(1),
            "expected single campaign pacing"
        );
        assert!(
            pacing["mean_kappa"].as_f64().expect("pacing mean kappa")
                <= config.budget_broker.max_kappa + f64::EPSILON
        );
        assert!(
            pacing["dual_price_max"].as_f64().expect("dual price max") >= 0.0,
            "dual price max should be non-negative"
        );
    }

    assert!(generated.iter().all(|ts| *ts > 0));
    for window in generated.windows(2) {
        if let [prev, next] = window {
            assert!(next >= prev);
        }
    }
    let mut latency_samples = latencies.clone();
    latency_samples.sort_unstable();
    let index = ((latency_samples.len() as f64) * 0.95).ceil() as usize;
    let p95 = latency_samples
        .get(index.saturating_sub(1))
        .copied()
        .unwrap_or_default();
    let max_latency = latency_samples.into_iter().max().unwrap_or_default();
    assert!(
        max_latency < 1_000_000,
        "proof verification exceeded 1s: {max_latency}"
    );
    assert!(
        p95 < 750_000,
        "p95 attestation latency should stay under 750ms: {p95}"
    );
}

#[testkit::tb_serial]
fn ad_market_committee_rejects_stale_snapshot() {
    let fixture = committee_fixture();
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    config.attestation.verifier_committee = Some(verifier_committee_config(&fixture));

    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_committee_stale_snapshot", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-committee",
            "advertiser_account": "adv-committee",
            "budget_usd_micros": 6000000,
            "creatives": [
                {
                    "id": "creative-committee",
                    "action_rate_ppm": 500000,
                    "margin_ppm": 700000,
                    "value_per_action_usd_micros": 1500000,
                    "max_cpi_usd_micros": 1500000,
                    "badges": [],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.bytes = 1_024;
    base_ctx.population_estimate = Some(96);
    base_ctx.verifier_committee = Some(fixture.receipt.clone());
    base_ctx.verifier_stake_snapshot = Some(fixture.snapshot.clone());
    base_ctx.verifier_transcript = fixture.transcript.clone();

    let key_probe = make_reservation_key(4_001);
    let outcome_probe = market
        .reserve_impression(key_probe.clone(), base_ctx.clone())
        .expect("probe reservation");
    let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
    market.cancel(&key_probe);

    let attestation = SelectionAttestation::Snark {
        proof: proof_bytes.clone(),
        circuit_id: SELECTION_CIRCUIT_ID.into(),
    };

    let mut ctx_stale = base_ctx.clone();
    ctx_stale.attestations = vec![attestation.clone()];
    let mut stale_snapshot = fixture.snapshot.clone();
    stale_snapshot.staking_epoch = stale_snapshot.staking_epoch.saturating_add(9);
    ctx_stale.verifier_stake_snapshot = Some(stale_snapshot);
    let key_stale = make_reservation_key(4_002);
    let outcome_stale = market
        .reserve_impression(key_stale.clone(), ctx_stale)
        .expect("stale snapshot reservation");
    assert_eq!(
        outcome_stale.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Missing,
        "stale stake snapshot should strip the attestation"
    );
    assert!(
        outcome_stale.selection_receipt.proof_metadata.is_none(),
        "stale stake snapshot should drop proof metadata"
    );
    market.cancel(&key_stale);

    let mut ctx_valid = base_ctx.clone();
    ctx_valid.attestations = vec![attestation];
    let key_valid = make_reservation_key(4_003);
    let outcome_valid = market
        .reserve_impression(key_valid.clone(), ctx_valid)
        .expect("valid reservation");
    market.cancel(&key_valid);
    assert_eq!(
        outcome_valid.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Snark
    );
}

#[testkit::tb_serial]
fn ad_market_committee_rejects_mismatched_transcript() {
    let fixture = committee_fixture();
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    config.attestation.verifier_committee = Some(verifier_committee_config(&fixture));

    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_committee_transcript", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-committee-transcript",
            "advertiser_account": "adv-committee",
            "budget_usd_micros": 4800000,
            "creatives": [
                {
                    "id": "creative-committee-transcript",
                    "action_rate_ppm": 520000,
                    "margin_ppm": 690000,
                    "value_per_action_usd_micros": 1400000,
                    "max_cpi_usd_micros": 1400000,
                    "badges": [],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.bytes = 896;
    base_ctx.population_estimate = Some(72);
    base_ctx.verifier_committee = Some(fixture.receipt.clone());
    base_ctx.verifier_stake_snapshot = Some(fixture.snapshot.clone());
    base_ctx.verifier_transcript = fixture.transcript.clone();

    let key_probe = make_reservation_key(4_101);
    let outcome_probe = market
        .reserve_impression(key_probe.clone(), base_ctx.clone())
        .expect("probe reservation");
    let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
    market.cancel(&key_probe);

    let attestation = SelectionAttestation::Snark {
        proof: proof_bytes.clone(),
        circuit_id: SELECTION_CIRCUIT_ID.into(),
    };

    let mut ctx_mismatched = base_ctx.clone();
    ctx_mismatched.attestations = vec![attestation.clone()];
    ctx_mismatched.verifier_transcript = b"unexpected-transcript".to_vec();
    let key_bad = make_reservation_key(4_102);
    let outcome_bad = market
        .reserve_impression(key_bad.clone(), ctx_mismatched)
        .expect("mismatched transcript reservation");
    assert_eq!(
        outcome_bad.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Missing,
        "mismatched transcript should strip attestation"
    );
    assert!(
        outcome_bad.selection_receipt.proof_metadata.is_none(),
        "mismatched transcript should drop proof metadata"
    );
    market.cancel(&key_bad);

    let mut ctx_valid = base_ctx.clone();
    ctx_valid.attestations = vec![attestation];
    let key_valid = make_reservation_key(4_103);
    let outcome_valid = market
        .reserve_impression(key_valid.clone(), ctx_valid)
        .expect("valid reservation");
    market.cancel(&key_valid);
    assert_eq!(
        outcome_valid.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Snark
    );
}

#[testkit::tb_serial]
fn ad_market_committee_blocks_invalid_when_attestation_required() {
    let fixture = committee_fixture();
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = true;
    config.attestation.verifier_committee = Some(verifier_committee_config(&fixture));

    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_committee_required", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-committee-required",
            "advertiser_account": "adv-committee",
            "budget_usd_micros": 7200000,
            "creatives": [
                {
                    "id": "creative-committee-required",
                    "action_rate_ppm": 540000,
                    "margin_ppm": 680000,
                    "value_per_action_usd_micros": 1450000,
                    "max_cpi_usd_micros": 1450000,
                    "badges": [],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let campaign = Campaign {
        id: "cmp-committee-required".into(),
        advertiser_account: "adv-committee".into(),
        budget_usd_micros: 7_200_000,
        creatives: vec![Creative {
            id: "creative-committee-required".into(),
            action_rate_ppm: 540_000,
            margin_ppm: 680_000,
            value_per_action_usd_micros: 1_450_000,
            max_cpi_usd_micros: Some(1_450_000),
            lift_ppm: 0,
            badges: Vec::new(),
            domains: vec!["example.test".into()],
            metadata: HashMap::new(),
        }],
        targeting: CampaignTargeting {
            domains: vec!["example.test".into()],
            badges: Vec::new(),
        },
        metadata: HashMap::new(),
    };

    let mut probe_config = config.clone();
    probe_config.attestation.require_attestation = false;
    let probe_market = InMemoryMarketplace::new(probe_config);
    probe_market
        .register_campaign(campaign.clone())
        .expect("probe campaign registered");

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.bytes = 2_048;
    base_ctx.population_estimate = Some(120);
    base_ctx.verifier_committee = Some(fixture.receipt.clone());
    base_ctx.verifier_stake_snapshot = Some(fixture.snapshot.clone());
    base_ctx.verifier_transcript = fixture.transcript.clone();

    let key_probe = make_reservation_key(4_101);
    let outcome_probe = probe_market
        .reserve_impression(key_probe.clone(), base_ctx.clone())
        .expect("probe reservation");
    let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
    probe_market.cancel(&key_probe);

    let attestation = SelectionAttestation::Snark {
        proof: proof_bytes.clone(),
        circuit_id: SELECTION_CIRCUIT_ID.into(),
    };

    let mut ctx_invalid = base_ctx.clone();
    ctx_invalid.attestations = vec![attestation.clone()];
    ctx_invalid.verifier_transcript = b"tampered-transcript".to_vec();
    let key_invalid = make_reservation_key(4_102);
    assert!(
        market
            .reserve_impression(key_invalid.clone(), ctx_invalid)
            .is_none(),
        "invalid transcript should block reservation when attestation required"
    );

    let mut ctx_valid = base_ctx.clone();
    ctx_valid.attestations = vec![attestation];
    let key_valid = make_reservation_key(4_103);
    let outcome_valid = market
        .reserve_impression(key_valid.clone(), ctx_valid)
        .expect("valid reservation");
    market.cancel(&key_valid);
    assert_eq!(
        outcome_valid.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Snark
    );
}

#[testkit::tb_serial]
fn ad_market_committee_rejects_weight_mismatch() {
    let fixture = committee_fixture();
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    config.attestation.verifier_committee = Some(verifier_committee_config(&fixture));

    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_committee_weight_mismatch", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-committee-weight",
            "advertiser_account": "adv-committee",
            "budget_usd_micros": 6800000,
            "creatives": [
                {
                    "id": "creative-committee-weight",
                    "action_rate_ppm": 520000,
                    "margin_ppm": 690000,
                    "value_per_action_usd_micros": 1480000,
                    "max_cpi_usd_micros": 1480000,
                    "badges": [],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.bytes = 1_024;
    base_ctx.population_estimate = Some(84);
    base_ctx.verifier_committee = Some(fixture.receipt.clone());
    base_ctx.verifier_stake_snapshot = Some(fixture.snapshot.clone());
    base_ctx.verifier_transcript = fixture.transcript.clone();

    let key_probe = make_reservation_key(4_301);
    let outcome_probe = market
        .reserve_impression(key_probe.clone(), base_ctx.clone())
        .expect("probe reservation");
    let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
    market.cancel(&key_probe);

    let attestation = SelectionAttestation::Snark {
        proof: proof_bytes.clone(),
        circuit_id: SELECTION_CIRCUIT_ID.into(),
    };

    let mut tampered_receipt = fixture.receipt.clone();
    if let Some(member) = tampered_receipt.committee.get_mut(0) {
        member.weight_ppm = member.weight_ppm.saturating_add(25_000);
    }

    let mut ctx_invalid = base_ctx.clone();
    ctx_invalid.attestations = vec![attestation.clone()];
    ctx_invalid.verifier_committee = Some(tampered_receipt);
    let key_invalid = make_reservation_key(4_302);
    let outcome_invalid = market
        .reserve_impression(key_invalid.clone(), ctx_invalid)
        .expect("invalid reservation");
    assert_eq!(
        outcome_invalid.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Missing,
        "tampered committee weight should strip attestation"
    );
    assert!(
        outcome_invalid.selection_receipt.proof_metadata.is_none(),
        "tampered committee weight should drop proof metadata"
    );
    market.cancel(&key_invalid);

    let mut ctx_valid = base_ctx.clone();
    ctx_valid.attestations = vec![attestation];
    let key_valid = make_reservation_key(4_303);
    let outcome_valid = market
        .reserve_impression(key_valid.clone(), ctx_valid)
        .expect("valid reservation");
    market.cancel(&key_valid);
    assert_eq!(
        outcome_valid.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Snark
    );
}

#[cfg(all(feature = "telemetry", feature = "python-bindings"))]
fn scrape_metrics(addr: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect metrics exporter");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("write metrics request");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read metrics response");
    String::from_utf8(buf).expect("metrics response utf8")
}

#[cfg(all(feature = "telemetry", feature = "python-bindings"))]
fn parse_committee_metric(metrics: &str, committee: &str, reason: &str) -> Option<f64> {
    for line in metrics.lines() {
        if line.starts_with("#") {
            continue;
        }
        if !line.starts_with("ad_verifier_committee_rejection_total{") {
            continue;
        }
        if !line.contains(&format!("committee=\"{committee}\"")) {
            continue;
        }
        if !line.contains(&format!("reason=\"{reason}\"")) {
            continue;
        }
        if let Some(value_str) = line.split_whitespace().last() {
            if let Ok(value) = value_str.parse::<f64>() {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(all(feature = "telemetry", feature = "python-bindings"))]
#[testkit::tb_serial]
fn ad_market_metrics_export_surfaces_committee_rejection_labels() {
    the_block::prepare_freethreaded_python().expect("python runtime initialised");
    let fixture = committee_fixture();
    the_block::reset_ad_verifier_committee_rejections();
    the_block::ensure_ad_verifier_committee_label(&fixture.policy.label, "snapshot_missing");
    let mut config = MarketplaceConfig::default();
    config.attestation.preferred_circuit_ids = {
        let mut set = HashSet::new();
        set.insert(SELECTION_CIRCUIT_ID.to_string());
        set
    };
    config.attestation.allow_tee_fallback = false;
    config.attestation.require_attestation = false;
    config.attestation.verifier_committee = Some(verifier_committee_config(&fixture));

    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_metrics_export", config.clone());

    let campaign_payload = parse_json(
        r#"{
            "id": "cmp-committee-metrics",
            "advertiser_account": "adv-committee",
            "budget_usd_micros": 6400000,
            "creatives": [
                {
                    "id": "creative-committee-metrics",
                    "action_rate_ppm": 530000,
                    "margin_ppm": 670000,
                    "value_per_action_usd_micros": 1420000,
                    "max_cpi_usd_micros": 1420000,
                    "badges": [],
                    "domains": ["example.test"],
                    "metadata": {}
                }
            ],
            "targeting": {
                "domains": ["example.test"],
                "badges": []
            },
            "metadata": {}
        }"#,
    );
    expect_ok(harness.call("ad_market.register_campaign", campaign_payload));

    let market = Arc::clone(&harness.market);
    let mut base_ctx = ImpressionContext::default();
    base_ctx.domain = "example.test".into();
    base_ctx.provider = Some("wallet".into());
    base_ctx.bytes = 1_024;
    base_ctx.population_estimate = Some(92);
    base_ctx.verifier_committee = Some(fixture.receipt.clone());
    base_ctx.verifier_stake_snapshot = Some(fixture.snapshot.clone());
    base_ctx.verifier_transcript = fixture.transcript.clone();

    let key_probe = make_reservation_key(7_001);
    let outcome_probe = market
        .reserve_impression(key_probe.clone(), base_ctx.clone())
        .expect("probe reservation");
    let proof_bytes = build_snark_proof(&outcome_probe.selection_receipt);
    market.cancel(&key_probe);

    let (addr, handle) =
        serve_metrics_with_shutdown("127.0.0.1:0").expect("start metrics exporter");
    let baseline_metrics = scrape_metrics(&addr);
    let baseline_metric =
        parse_committee_metric(&baseline_metrics, &fixture.policy.label, "snapshot_missing");
    if let Some(value) = baseline_metric {
        assert!(
            value == 0.0,
            "expected rejection counter to start at zero, found {value}"
        );
    }
    let baseline_value = baseline_metric.unwrap_or(0.0);

    let mut ctx_missing_snapshot = base_ctx.clone();
    ctx_missing_snapshot.verifier_stake_snapshot = None;
    ctx_missing_snapshot.verifier_committee = Some(fixture.receipt.clone());
    ctx_missing_snapshot.verifier_transcript = fixture.transcript.clone();
    ctx_missing_snapshot.attestations = vec![SelectionAttestation::Snark {
        proof: proof_bytes.clone(),
        circuit_id: SELECTION_CIRCUIT_ID.into(),
    }];

    let key_missing = make_reservation_key(7_002);
    let outcome_missing = market
        .reserve_impression(key_missing.clone(), ctx_missing_snapshot)
        .expect("missing snapshot reservation");
    market.cancel(&key_missing);
    assert_eq!(
        outcome_missing.selection_receipt.attestation_kind(),
        SelectionAttestationKind::Missing,
        "missing snapshot should prevent attestation attachment",
    );

    let after_metrics = scrape_metrics(&addr);
    handle.shutdown();
    let after_value =
        parse_committee_metric(&after_metrics, &fixture.policy.label, "snapshot_missing")
            .unwrap_or_else(|| {
                panic!(
                    "committee rejection metric missing after failure. payload:\n{}",
                    after_metrics
                )
            });
    let delta = after_value - baseline_value;
    assert!(
        (delta - 1.0).abs() < f64::EPSILON,
        "expected exactly one rejection increment (baseline={baseline_value}, after={after_value})"
    );
}

#[testkit::tb_serial]
fn rpc_record_conversion_success() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_rpc", config);
    let baseline_budget = expect_ok(harness.call("ad_market.budget", Value::Null));
    let baseline_summary = baseline_budget
        .get("conversion_summary")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let baseline_authenticated = baseline_summary
        .get("authenticated_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let creative = Creative {
        id: "creative-rpc".into(),
        action_rate_ppm: 150_000,
        margin_ppm: 900_000,
        value_per_action_usd_micros: 500_000,
        max_cpi_usd_micros: None,
        lift_ppm: 0,
        badges: Vec::new(),
        domains: Vec::new(),
        metadata: HashMap::new(),
        mesh_payload: None,
        placement: Default::default(),
    };
    let advertiser_token = "adv-rpc-token";
    let token_hash = hex::encode(blake3::hash(advertiser_token.as_bytes()).as_bytes());
    let mut metadata = HashMap::new();
    metadata.insert("conversion_token_hash".into(), token_hash);
    let campaign = Campaign {
        id: "cmp-rpc".into(),
        advertiser_account: "adv-rpc".into(),
        budget_usd_micros: 8_000_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata,
    };
    harness
        .market
        .register_campaign(campaign)
        .expect("campaign registered");
    let params = json_mod::json!({
        "campaign_id": "cmp-rpc",
        "creative_id": "creative-rpc",
        "advertiser_account": "adv-rpc",
        "assignment": {
            "fold": 0,
            "in_holdout": false,
            "propensity": 1.0
        },
        "value_usd_micros": 250_000,
        "occurred_at_micros": 777_000
    });
    let response = harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some(format!("Advertiser adv-rpc:{advertiser_token}")),
    );
    let value = expect_ok(response);
    assert_eq!(value["status"], Value::String("ok".into()));
    let summary = value
        .get("conversion_summary")
        .and_then(Value::as_object)
        .cloned()
        .expect("conversion summary in response");
    let updated_total = summary
        .get("authenticated_total")
        .and_then(Value::as_u64)
        .expect("authenticated_total present");
    assert!(
        updated_total >= baseline_authenticated + 1,
        "authenticated_total should increment"
    );
    let budget_after = expect_ok(harness.call("ad_market.budget", Value::Null));
    let budget_summary = budget_after
        .get("conversion_summary")
        .and_then(Value::as_object)
        .cloned()
        .expect("conversion summary in budget");
    assert_eq!(
        budget_summary
            .get("authenticated_total")
            .and_then(Value::as_u64),
        Some(updated_total)
    );
}

#[testkit::tb_serial]
fn rpc_record_conversion_rejects_unknown_creative() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_unknown", config);
    let params = json_mod::json!({
        "campaign_id": "missing-cmp",
        "creative_id": "missing-creative",
        "advertiser_account": "missing-adv",
        "assignment": {
            "fold": 1,
            "in_holdout": true,
            "propensity": 0.5
        }
    });
    let error = expect_error(harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some("Advertiser missing-adv:token".into()),
    ));
    assert_eq!(error.code(), -32001);
}

#[testkit::tb_serial]
fn mesh_holdout_treatment_settlement_has_mesh_fields() {
    let mut config = MarketplaceConfig::default();
    config.oracles.prices.ct_price_usd_micros = 1_000_000;
    config.oracles.prices.it_price_usd_micros = 1_000_000;
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_mesh_holdout_treatment", config);
    let market = harness
        .in_memory_market
        .clone()
        .expect("in-memory market available");

    let mesh_payload: Vec<u8> = vec![0xAB, 0xCD, 0xEF, 0x01];
    let creative = Creative {
        id: "creative-mesh".into(),
        action_rate_ppm: 400_000,
        margin_ppm: 800_000,
        value_per_action_usd_micros: 750_000,
        max_cpi_usd_micros: None,
        lift_ppm: 150_000,
        badges: vec![],
        domains: vec!["mesh.test".into()],
        metadata: HashMap::new(),
        mesh_payload: Some(mesh_payload.clone()),
        placement: CreativePlacement {
            mesh_enabled: true,
            mesh_only: false,
            allowed_channels: vec![DeliveryChannel::Mesh],
        },
    };
    let campaign = Campaign {
        id: "cmp-mesh".into(),
        advertiser_account: "mesh-advertiser".into(),
        budget_usd_micros: 5_000_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata: HashMap::new(),
    };
    market
        .register_campaign(campaign)
        .expect("campaign registered");

    let mut ctx = ImpressionContext::default();
    ctx.domain = "mesh.test".into();
    ctx.provider = Some("mesh-gateway".into());
    ctx.bytes = 256;
    ctx.population_estimate = Some(250);
    ctx.delivery_channel = DeliveryChannel::Mesh;
    ctx.mesh = Some(MeshContext {
        peer_id: Some("mesh-peer-1".into()),
        transport: Some("tcp".into()),
        latency_ms: Some(42),
        hop_proofs: vec!["hop-01".into(), "hop-02".into()],
    });

    let mut holdout_payload: Option<Vec<u8>> = None;
    let mut holdout_digest: Option<String> = None;
    let mut holdout_mesh_peer = None;
    let mut treatment_payload: Option<Vec<u8>> = None;
    let mut treatment_digest: Option<String> = None;
    let mut treatment_settlement = None;

    for seed in 10_000..20_000u64 {
        let key = make_reservation_key(seed);
        if let Some(outcome) = market.reserve_impression(key.clone(), ctx.clone()) {
            let Some(payload) = outcome.mesh_payload.clone() else {
                market.cancel(&key);
                continue;
            };
            let digest = hex::encode(blake3::hash(&payload).as_bytes());
            if outcome.uplift_assignment.in_holdout {
                holdout_payload = Some(payload);
                holdout_digest = Some(digest);
                holdout_mesh_peer = outcome.selection_receipt.cohort.mesh_peer.clone();
                assert!(market.commit(&key).is_none());
            } else {
                let settlement = market.commit(&key).expect("treatment settlement produced");
                treatment_payload = settlement.mesh_payload.clone();
                treatment_digest = settlement.mesh_payload_digest.clone();
                treatment_settlement = Some(settlement);
            }
        }
        if holdout_payload.is_some() && treatment_settlement.is_some() {
            break;
        }
    }

    let settlement = treatment_settlement.expect("treatment settlement found");
    let holdout_payload = holdout_payload.expect("holdout payload captured");
    let holdout_digest = holdout_digest.expect("holdout digest computed");
    let treatment_payload = treatment_payload.expect("treatment payload present");
    let treatment_digest = treatment_digest.expect("treatment digest present");

    let expected_digest = hex::encode(blake3::hash(&mesh_payload).as_bytes());
    assert_eq!(holdout_payload, mesh_payload);
    assert_eq!(treatment_payload, mesh_payload);
    assert_eq!(treatment_digest, expected_digest);
    assert_eq!(holdout_digest, expected_digest);
    assert_eq!(
        settlement.mesh_payload_digest.as_deref(),
        Some(expected_digest.as_str())
    );
    assert_eq!(settlement.delivery_channel, DeliveryChannel::Mesh);
    assert!(settlement.mesh_payload.is_some());
    assert_eq!(
        settlement.selection_receipt.cohort.mesh_peer.as_deref(),
        Some("mesh-peer-1")
    );
    assert_eq!(holdout_mesh_peer.as_deref(), Some("mesh-peer-1"));
}

#[testkit::tb_serial]
fn rpc_record_conversion_requires_authorization_header() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_no_auth", config);
    let baseline_budget = expect_ok(harness.call("ad_market.budget", Value::Null));
    let baseline_errors = baseline_budget
        .get("conversion_summary")
        .and_then(Value::as_object)
        .and_then(|summary| summary.get("error_counts"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let baseline_auth_required = baseline_errors
        .get("auth_required")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let creative = Creative {
        id: "creative-auth".into(),
        action_rate_ppm: 120_000,
        margin_ppm: 800_000,
        value_per_action_usd_micros: 400_000,
        max_cpi_usd_micros: None,
        lift_ppm: 0,
        badges: Vec::new(),
        domains: Vec::new(),
        metadata: HashMap::new(),
        mesh_payload: None,
        placement: Default::default(),
    };
    let token_hash = hex::encode(blake3::hash(b"adv-auth-token").as_bytes());
    let mut metadata = HashMap::new();
    metadata.insert("conversion_token_hash".into(), token_hash);
    let campaign = Campaign {
        id: "cmp-auth".into(),
        advertiser_account: "adv-auth".into(),
        budget_usd_micros: 5_000_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata,
    };
    harness
        .market
        .register_campaign(campaign)
        .expect("campaign registered");
    let params = json_mod::json!({
        "campaign_id": "cmp-auth",
        "creative_id": "creative-auth",
        "advertiser_account": "adv-auth",
        "assignment": {
            "fold": 0,
            "in_holdout": false,
            "propensity": 1.0
        }
    });
    let error = expect_error(harness.call("ad_market.record_conversion", params));
    assert_eq!(error.code(), -32030);
    let budget_after = expect_ok(harness.call("ad_market.budget", Value::Null));
    let after_errors = budget_after
        .get("conversion_summary")
        .and_then(Value::as_object)
        .and_then(|summary| summary.get("error_counts"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let after_auth_required = after_errors
        .get("auth_required")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    assert!(
        after_auth_required >= baseline_auth_required + 1,
        "auth_required error counter should increment"
    );
}

#[testkit::tb_serial]
fn rpc_record_conversion_rejects_invalid_token() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_bad_token", config);
    let creative = Creative {
        id: "creative-bad-token".into(),
        action_rate_ppm: 100_000,
        margin_ppm: 850_000,
        value_per_action_usd_micros: 350_000,
        max_cpi_usd_micros: None,
        lift_ppm: 0,
        badges: Vec::new(),
        domains: Vec::new(),
        metadata: HashMap::new(),
        mesh_payload: None,
        placement: Default::default(),
    };
    let valid_hash = hex::encode(blake3::hash(b"correct-token").as_bytes());
    let mut metadata = HashMap::new();
    metadata.insert("conversion_token_hash".into(), valid_hash);
    let campaign = Campaign {
        id: "cmp-bad-token".into(),
        advertiser_account: "adv-bad".into(),
        budget_usd_micros: 6_000_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata,
    };
    harness
        .market
        .register_campaign(campaign)
        .expect("campaign registered");
    let params = json_mod::json!({
        "campaign_id": "cmp-bad-token",
        "creative_id": "creative-bad-token",
        "advertiser_account": "adv-bad",
        "assignment": {
            "fold": 1,
            "in_holdout": false,
            "propensity": 0.8
        }
    });
    let error = expect_error(harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some("Advertiser adv-bad:wrong-token".into()),
    ));
    assert_eq!(error.code(), -32033);
}

#[testkit::tb_serial]
fn rpc_record_conversion_rejects_account_mismatch() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_account_mismatch", config);
    let creative = Creative {
        id: "creative-mismatch".into(),
        action_rate_ppm: 110_000,
        margin_ppm: 820_000,
        value_per_action_usd_micros: 360_000,
        max_cpi_usd_micros: None,
        lift_ppm: 0,
        badges: Vec::new(),
        domains: Vec::new(),
        metadata: HashMap::new(),
        mesh_payload: None,
        placement: Default::default(),
    };
    let hash = hex::encode(blake3::hash(b"mismatch-token").as_bytes());
    let mut metadata = HashMap::new();
    metadata.insert("conversion_token_hash".into(), hash);
    let campaign = Campaign {
        id: "cmp-mismatch".into(),
        advertiser_account: "adv-match".into(),
        budget_usd_micros: 4_500_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata,
    };
    harness
        .market
        .register_campaign(campaign)
        .expect("campaign registered");
    let params = json_mod::json!({
        "campaign_id": "cmp-mismatch",
        "creative_id": "creative-mismatch",
        "advertiser_account": "adv-match",
        "assignment": {
            "fold": 2,
            "in_holdout": true,
            "propensity": 0.4
        }
    });
    let error = expect_error(harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some("Advertiser adv-other:mismatch-token".into()),
    ));
    assert_eq!(error.code(), -32031);
}

#[testkit::tb_serial]
fn rpc_record_conversion_rejects_missing_token_metadata() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_missing_token", config);
    let creative = Creative {
        id: "creative-missing-token".into(),
        action_rate_ppm: 95_000,
        margin_ppm: 780_000,
        value_per_action_usd_micros: 300_000,
        max_cpi_usd_micros: None,
        lift_ppm: 0,
        badges: Vec::new(),
        domains: Vec::new(),
        metadata: HashMap::new(),
        mesh_payload: None,
        placement: Default::default(),
    };
    let campaign = Campaign {
        id: "cmp-missing-token".into(),
        advertiser_account: "adv-missing".into(),
        budget_usd_micros: 3_500_000,
        creatives: vec![creative],
        targeting: CampaignTargeting::default(),
        metadata: HashMap::new(),
    };
    harness
        .market
        .register_campaign(campaign)
        .expect("campaign registered");
    let params = json_mod::json!({
        "campaign_id": "cmp-missing-token",
        "creative_id": "creative-missing-token",
        "advertiser_account": "adv-missing",
        "assignment": {
            "fold": 3,
            "in_holdout": false,
            "propensity": 0.6
        }
    });
    let error = expect_error(harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some("Advertiser adv-missing:any".into()),
    ));
    assert_eq!(error.code(), -32032);
}

#[testkit::tb_serial]
fn rpc_record_conversion_rejects_malformed_payload() {
    let config = MarketplaceConfig::default();
    let (_dir, harness, _readiness) =
        build_in_memory_harness("ad_market_record_conversion_malformed", config);
    let params = json_mod::json!({
        "campaign_id": "cmp-malformed",
        "creative_id": "creative-malformed",
        "advertiser_account": "adv-malformed"
    });
    let error = expect_error(harness.call_with_auth(
        "ad_market.record_conversion",
        params,
        Some("Advertiser adv-malformed:token".into()),
    ));
    assert_eq!(error.code(), -32602);
}
