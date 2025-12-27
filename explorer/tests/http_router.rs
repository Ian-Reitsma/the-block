use explorer::{router, BlockPayoutBreakdown, Explorer, ExplorerHttpState};
use foundation_serialization::json;
use foundation_sqlite::{params, Connection};
use httpd::StatusCode;
use std::sync::Arc;
use sys::tempfile;

#[test]
fn block_lookup_via_router() {
    runtime::block_on(async {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("explorer.db");
        let explorer = Arc::new(Explorer::open(&db_path).expect("open explorer"));

        let height = 7u64;
        let base_fee = 5u64;
        let read_total = 600u64;
        let read_viewer = 250u64;
        let read_host = 150u64;
        let read_hardware = 75u64;
        let read_verifier = 50u64;
        let read_liquidity = 25u64;
        let ad_viewer = 12u64;
        let ad_host = 10u64;
        let ad_hardware = 4u64;
        let ad_verifier = 3u64;
        let ad_liquidity = 1u64;
        let ad_miner = 2u64;
        let ad_host_it = 6u64;
        let ad_hardware_it = 4u64;
        let ad_verifier_it = 2u64;
        let ad_liquidity_it = 1u64;
        let ad_miner_it = 1u64;
        let ad_total_usd_micros = 77_000u64;
        let ad_settlement_count = 3u64;
        let ad_ct_price = 1_100_000u64;
        let ad_it_price = 950_000u64;
        let zero_array = format!("{:?}", [0u8; 32]);

        let block_json = format!(
            r#"{{
                "index": {height},
                "previous_hash": "prev",
                "timestamp_millis": 42,
                "transactions": [],
                "difficulty": 9,
                "retune_hint": 0,
                "nonce": 123,
                "hash": "b1",
                "coinbase_block": 0,
                "coinbase_industrial": 0,
                "storage_sub_ct": 0,
                "read_sub_ct": {read_total},
                "read_sub_viewer_ct": {read_viewer},
                "read_sub_host_ct": {read_host},
                "read_sub_hardware_ct": {read_hardware},
                "read_sub_verifier_ct": {read_verifier},
                "read_sub_liquidity_ct": {read_liquidity},
                "ad_viewer": {ad_viewer},
                "ad_host": {ad_host},
                "ad_hardware": {ad_hardware},
                "ad_verifier": {ad_verifier},
                "ad_liquidity": {ad_liquidity},
                "ad_miner": {ad_miner},
                "ad_host_it": {ad_host_it},
                "ad_hardware_it": {ad_hardware_it},
                "ad_verifier_it": {ad_verifier_it},
                "ad_liquidity_it": {ad_liquidity_it},
                "ad_miner_it": {ad_miner_it},
                "ad_total_usd_micros": {ad_total_usd_micros},
                "ad_settlement_count": {ad_settlement_count},
                "ad_oracle_price_usd_micros": {ad_ct_price},
                "ad_oracle_it_price_usd_micros": {ad_it_price},
                "compute_sub_ct": 0,
                "proof_rebate_ct": 0,
                "storage_sub_it": 0,
                "read_sub_it": 0,
                "compute_sub_it": 0,
                "read_root": {zero_array},
                "fee_checksum": "",
                "state_root": "",
                "base_fee": {base_fee},
                "l2_roots": [],
                "l2_sizes": [],
                "vdf_commit": {zero_array},
                "vdf_output": {zero_array},
                "vdf_proof": []
            }}"#
        );

        let conn = Connection::open(&db_path).expect("open sqlite");
        conn.execute(
            "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
            params!["b1", height as i64, block_json.as_bytes()],
        )
        .expect("insert block");

        let app = router(ExplorerHttpState::new(explorer));
        let payout_response = app
            .handle(app.request_builder().path("/blocks/b1/payouts").build())
            .await
            .expect("payout response");
        assert_eq!(payout_response.status(), StatusCode::OK);
        let payload: Option<json::Value> =
            json::from_slice(payout_response.body()).expect("decode payouts");
        let payouts_json = payload.expect("payout breakdown present");
        let payouts = BlockPayoutBreakdown::from_json_map(&payouts_json).expect("payout json map");
        assert_eq!(payouts.hash, "b1");
        assert_eq!(payouts.height, height);
        assert_eq!(payouts.read_subsidy.total_ct, read_total);
        assert_eq!(payouts.read_subsidy.viewer_ct, read_viewer);
        assert_eq!(payouts.read_subsidy.host_ct, read_host);
        assert_eq!(payouts.read_subsidy.hardware_ct, read_hardware);
        assert_eq!(payouts.read_subsidy.verifier_ct, read_verifier);
        assert_eq!(payouts.read_subsidy.liquidity_ct, read_liquidity);
        assert_eq!(
            payouts.read_subsidy.miner_ct,
            read_total - (read_viewer + read_host + read_hardware + read_verifier + read_liquidity)
        );
        assert_eq!(
            payouts.advertising.total_ct,
            ad_viewer + ad_host + ad_hardware + ad_verifier + ad_liquidity + ad_miner
        );
        assert_eq!(
            payouts.advertising.total_it,
            ad_host_it + ad_hardware_it + ad_verifier_it + ad_liquidity_it + ad_miner_it
        );
        assert_eq!(payouts.advertising.viewer_ct, ad_viewer);
        assert_eq!(payouts.advertising.host_ct, ad_host);
        assert_eq!(payouts.advertising.hardware_ct, ad_hardware);
        assert_eq!(payouts.advertising.verifier_ct, ad_verifier);
        assert_eq!(payouts.advertising.liquidity_ct, ad_liquidity);
        assert_eq!(payouts.advertising.miner_ct, ad_miner);
        assert_eq!(payouts.advertising.viewer_it, 0);
        assert_eq!(payouts.advertising.host_it, ad_host_it);
        assert_eq!(payouts.advertising.hardware_it, ad_hardware_it);
        assert_eq!(payouts.advertising.verifier_it, ad_verifier_it);
        assert_eq!(payouts.advertising.liquidity_it, ad_liquidity_it);
        assert_eq!(payouts.advertising.miner_it, ad_miner_it);
        assert_eq!(payouts.total_usd_micros, ad_total_usd_micros);
        assert_eq!(payouts.settlement_count, ad_settlement_count);
        assert_eq!(payouts.price_usd_micros, ad_ct_price);
        assert_eq!(payouts.it_price_usd_micros, ad_it_price);
        assert!(payouts.treasury_events.is_empty());
    });
}
