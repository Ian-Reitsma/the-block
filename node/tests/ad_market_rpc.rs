#![cfg(feature = "integration-tests")]

use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ad_market::{
    DistributionPolicy, MarketplaceConfig, MarketplaceHandle, SledMarketplace, MICROS_PER_DOLLAR,
};
use foundation_rpc::{Request as RpcRequest, Response, RpcError};
use foundation_serialization::json::{self as json_mod, Value};
use the_block::{
    ad_readiness::{AdReadinessConfig, AdReadinessHandle},
    identity::{handle_registry::HandleRegistry, DidRegistry},
    rpc::{fuzz_dispatch_request, fuzz_runtime_config_with_admin, RpcRuntimeConfig},
    Blockchain,
};

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
    admin_token: String,
    readiness: Option<AdReadinessHandle>,
}

impl RpcHarness {
    fn call(&self, method: &str, params: Value) -> Response {
        let request = RpcRequest::new(method.to_string(), params);
        let auth_header = format!("Bearer {}", self.admin_token);
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
    assert_eq!(cohorts.len(), 1);
    let cohort_entry = cohorts[0].as_object().expect("cohort entry");
    assert_eq!(cohort_entry["observed_utilization_ppm"].as_u64(), Some(0));

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
    assert_eq!(utilization_initial["cohort_count"].as_u64(), Some(1));
    assert_eq!(utilization_initial["mean_ppm"].as_u64(), Some(0));
    assert_eq!(utilization_initial["max_ppm"].as_u64(), Some(0));
    let util_cohorts = utilization_initial["cohorts"]
        .as_array()
        .expect("cohort util");
    assert_eq!(util_cohorts.len(), 1);
    let util_entry = util_cohorts[0].as_object().expect("util entry");
    assert_eq!(util_entry["domain"].as_str(), Some("example.test"));
    assert_eq!(util_entry["observed_utilization_ppm"].as_u64(), Some(0));
    assert_eq!(util_entry["delta_utilization_ppm"].as_i64(), Some(0));

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
    assert_eq!(
        readiness_ready["ct_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    assert_eq!(
        readiness_ready["it_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    let oracle = readiness_ready["oracle"].as_object().expect("oracle map");
    let snapshot_oracle = oracle["snapshot"].as_object().expect("snapshot oracle");
    assert_eq!(
        snapshot_oracle["ct_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    assert_eq!(
        snapshot_oracle["it_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    let market_oracle = oracle["market"].as_object().expect("market oracle");
    assert_eq!(
        market_oracle["ct_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    assert_eq!(
        market_oracle["it_price_usd_micros"].as_u64(),
        Some(MICROS_PER_DOLLAR)
    );
    let utilization_ready = readiness_ready
        .get("utilization")
        .and_then(Value::as_object)
        .expect("utilization map");
    assert_eq!(utilization_ready["cohort_count"].as_u64(), Some(1));
    let ready_cohorts = utilization_ready["cohorts"]
        .as_array()
        .expect("ready cohorts");
    assert_eq!(ready_cohorts.len(), 1);
    let ready_entry = ready_cohorts[0].as_object().expect("ready entry");
    assert_eq!(ready_entry["observed_utilization_ppm"].as_u64(), Some(0));
    assert_eq!(ready_entry["delta_utilization_ppm"].as_i64(), Some(0));

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
    let sled = SledMarketplace::open(&market_dir, distribution).expect("market opened");
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
