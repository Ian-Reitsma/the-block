use ad_market::{InMemoryMarketplace, MarketplaceConfig, MarketplaceHandle};
use crypto_suite::{
    encoding::hex,
    hashing::blake3,
    signatures::ed25519::{Signature as EdSignature, VerifyingKey},
};
use explorer::{router, Explorer, ExplorerHttpState};
use foundation_serialization::json::{self, Value as JsonValue};
use httpd::StatusCode;
use the_block::ad_policy_snapshot::persist_snapshot;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use sys::tempfile::TempDir;

fn tmp_path(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

fn decode_json(body: &[u8]) -> JsonValue {
    json::value_from_slice(body).expect("decode json")
}

#[test]
fn ad_policy_snapshots_empty() {
    runtime::block_on(async {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp_path(&tmp, "explorer.sqlite");
        let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));
        let app = router(ExplorerHttpState::new(explorer));

        // No files under this node-data yet; expect empty list
        let data_dir = tmp_path(&tmp, "node-data");
        let url = format!(
            "/ad/policy/snapshots?data_dir={}",
            data_dir.to_string_lossy()
        );
        let resp = app.handle(app.request_builder().path(&url).build()).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let value = decode_json(resp.body());
        let obj = value.as_object().expect("object");
        let snaps = obj
            .get("snapshots")
            .and_then(JsonValue::as_array)
            .expect("snapshots array");
        assert!(snaps.is_empty());
    });
}

#[test]
fn ad_policy_snapshot_with_attestation() {
    runtime::block_on(async {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp_path(&tmp, "explorer.sqlite");
        let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));
        let app = router(ExplorerHttpState::new(explorer));

        let data_dir = tmp_path(&tmp, "node-data");
        let base = data_dir.to_string_lossy();

        // Prepare market and write a signed snapshot
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(MarketplaceConfig::default()));
        // fixed 32-byte hex key (0x07 repeated)
        let key_hex = hex::encode([7u8; 32]);
        std::env::set_var("TB_NODE_KEY_HEX", key_hex);
        persist_snapshot(&base, &market, 42).expect("persist snapshot");
        std::env::remove_var("TB_NODE_KEY_HEX");

        // Fetch detail endpoint
        let url = format!(
            "/ad/policy/snapshots/42?data_dir={}",
            data_dir.to_string_lossy()
        );
        let resp = app.handle(app.request_builder().path(&url).build()).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let value = decode_json(resp.body());
        let obj = value.as_object().expect("object");

        // Validate summary keys
        assert_eq!(obj.get("epoch").and_then(JsonValue::as_u64), Some(42));
        assert!(obj.get("generated_at").and_then(JsonValue::as_u64).is_some());
        let dist = obj.get("distribution").and_then(JsonValue::as_object).expect("distribution");
        assert!(dist.get("viewer_percent").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("host_percent").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("hardware_percent").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("verifier_percent").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("liquidity_percent").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("liquidity_split_ct_ppm").and_then(JsonValue::as_u64).is_some());
        assert!(dist.get("dual_token_settlement_enabled").and_then(JsonValue::as_bool).is_some());
        // medians present
        let med = obj.get("medians").and_then(JsonValue::as_object).expect("medians");
        assert!(med.get("storage_price_per_mib_usd_micros").and_then(JsonValue::as_u64).is_some());
        assert!(med.get("verifier_cost_usd_micros").and_then(JsonValue::as_u64).is_some());
        assert!(med.get("host_fee_usd_micros").and_then(JsonValue::as_u64).is_some());

        // Attestation present and valid
        let att = obj.get("attestation").and_then(JsonValue::as_object).expect("attestation");
        let pub_hex = att.get("pubkey_hex").and_then(JsonValue::as_str).expect("pubkey_hex");
        let sig_hex = att.get("signature_hex").and_then(JsonValue::as_str).expect("signature_hex");
        let hash_hex = att
            .get("payload_hash_hex")
            .and_then(JsonValue::as_str)
            .expect("payload_hash_hex");

        // Re-hash on-disk JSON payload to match sidecar
        let json_path = data_dir.join("ad_policy").join("42.json");
        let payload = fs::read(&json_path).expect("read snapshot");
        let digest = blake3::hash(&payload);
        assert_eq!(hash_hex, digest.to_hex().to_string());

        // Verify signature
        let pub_bytes: [u8; 32] = hex::decode(pub_hex).expect("decode pubkey").try_into().expect("pk len");
        let sig_bytes: [u8; 64] = hex::decode(sig_hex).expect("decode sig").try_into().expect("sig len");
        let vk = VerifyingKey::from_bytes(&pub_bytes).expect("verifying key");
        let sig = EdSignature::from_bytes(&sig_bytes);
        vk.verify(digest.as_bytes(), &sig).expect("signature valid");
    });
}

#[test]
fn ad_policy_snapshots_pagination_and_bounds() {
    runtime::block_on(async {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp_path(&tmp, "explorer.sqlite");
        let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));
        let app = router(ExplorerHttpState::new(explorer));

        let data_dir = tmp_path(&tmp, "node-data");
        let base = data_dir.to_string_lossy();
        let market: MarketplaceHandle = Arc::new(InMemoryMarketplace::new(MarketplaceConfig::default()));
        // Write four epochs
        for e in 1..=4u64 {
            persist_snapshot(&base, &market, e).expect("persist snapshot");
        }

        // Request bounded window [2,4] with limit 2 -> expect [4,3]
        let url = format!(
            "/ad/policy/snapshots?data_dir={}&start_epoch=2&end_epoch=4&limit=2",
            data_dir.to_string_lossy()
        );
        let resp = app.handle(app.request_builder().path(&url).build()).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let value = decode_json(resp.body());
        let obj = value.as_object().expect("object");
        let snaps = obj
            .get("snapshots")
            .and_then(JsonValue::as_array)
            .expect("snapshots array");
        assert_eq!(snaps.len(), 2);
        let first_epoch = snaps[0].as_object().and_then(|o| o.get("epoch")).and_then(JsonValue::as_u64).unwrap();
        let second_epoch = snaps[1].as_object().and_then(|o| o.get("epoch")).and_then(JsonValue::as_u64).unwrap();
        assert_eq!((first_epoch, second_epoch), (4, 3));

        // Edge cases: bad epoch param -> 400, missing file -> 404
        let bad = app
            .handle(app.request_builder().path("/ad/policy/snapshots/not-a-number").build())
            .await
            .expect("response");
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let not_found = app
            .handle(app.request_builder().path(&format!(
                "/ad/policy/snapshots/999999?data_dir={}",
                data_dir.to_string_lossy()
            )).build())
            .await
            .expect("response");
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    });
}

#[test]
fn ad_readiness_status_stitches_governance() {
    runtime::block_on(async {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp_path(&tmp, "explorer.sqlite");
        let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));
        let app = router(ExplorerHttpState::new(explorer));

        // Create readiness storage under node-data/ad_readiness
        let data_dir = tmp_path(&tmp, "node-data");
        let readiness_dir = data_dir.join("ad_readiness");
        fs::create_dir_all(&readiness_dir).expect("mkdir readiness");
        // Initialize files via handle (ensures default snapshot + config exist)
        let _handle = the_block::ad_readiness::AdReadinessHandle::open_with_storage(
            readiness_dir.to_str().unwrap(),
            the_block::ad_readiness::AdReadinessConfig::default(),
        );

        // Governance param history with rehearsal flags and percentile settings
        let gov_root = tmp_path(&tmp, "gov_state");
        let hist_dir = gov_root.join("governance/history");
        fs::create_dir_all(&hist_dir).expect("mkdir gov history");
        let changes = r#"[
            {"key":"AdRehearsalEnabled","new_value":1,"epoch":1},
            {"key":"AdRehearsalStabilityWindows","new_value":6,"epoch":1},
            {"key":"AdUsePercentileThresholds","new_value":1,"epoch":1},
            {"key":"AdViewerPercentile","new_value":75,"epoch":1},
            {"key":"AdHostPercentile","new_value":80,"epoch":1},
            {"key":"AdProviderPercentile","new_value":90,"epoch":1},
            {"key":"AdEmaSmoothingPpm","new_value":25000,"epoch":1},
            {"key":"AdFloorUniqueViewers","new_value":10,"epoch":1},
            {"key":"AdFloorHostCount","new_value":2,"epoch":1},
            {"key":"AdFloorProviderCount","new_value":1,"epoch":1},
            {"key":"AdCapUniqueViewers","new_value":100000,"epoch":1},
            {"key":"AdCapHostCount","new_value":1000,"epoch":1},
            {"key":"AdCapProviderCount","new_value":500,"epoch":1},
            {"key":"AdPercentileBuckets","new_value":60,"epoch":1}
        ]"#;
        fs::write(hist_dir.join("param_changes.json"), changes).expect("write param history");

        let url = format!(
            "/ad/readiness/status?data_dir={}&state={}&rehearsal_enabled=1&rehearsal_windows=6",
            data_dir.to_string_lossy(),
            gov_root.to_string_lossy()
        );
        let resp = app.handle(app.request_builder().path(&url).build()).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let value = decode_json(resp.body());
        let obj = value.as_object().expect("object");

        // rehearsal flags present and typed
        assert!(obj.get("rehearsal_enabled").and_then(JsonValue::as_bool).is_some());
        assert!(obj
            .get("rehearsal_required_windows")
            .and_then(JsonValue::as_u64)
            .is_some());

        // config shape and dynamic thresholds present
        let cfg = obj.get("config").and_then(JsonValue::as_object).expect("config");
        assert!(cfg.get("window_secs").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("min_unique_viewers").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("min_host_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("min_provider_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("use_percentile_thresholds").and_then(JsonValue::as_bool).is_some());
        assert!(cfg.get("viewer_percentile").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("host_percentile").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("provider_percentile").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("ema_smoothing_ppm").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("floor_unique_viewers").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("floor_host_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("floor_provider_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("cap_unique_viewers").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("cap_host_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("cap_provider_count").and_then(JsonValue::as_u64).is_some());
        assert!(cfg.get("percentile_buckets").and_then(JsonValue::as_u64).is_some());

        // snapshot present with expected keys
        let snap = obj.get("snapshot").and_then(JsonValue::as_object).expect("snapshot");
        for key in [
            "unique_viewers",
            "host_count",
            "provider_count",
            "ready",
            "last_updated",
            "total_usd_micros",
            "settlement_count",
            "ct_price_usd_micros",
            "it_price_usd_micros",
            "market_ct_price_usd_micros",
            "market_it_price_usd_micros",
            "ready_streak_windows",
        ] {
            assert!(snap.get(key).is_some(), "missing snapshot key: {}", key);
        }
        assert!(snap.get("blockers").is_some());
        // utilization_summary may be null or object; just ensure key exists
        assert!(snap.contains_key("utilization_summary"));
    });
}
