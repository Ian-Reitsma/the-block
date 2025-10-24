use explorer::Explorer;
use foundation_sqlite::{params, Connection};
use sys::tempfile;

#[test]
fn block_lookup_and_payout_breakdown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");

    let height = 1u64;
    let read_total = 1_000u64;
    let read_viewer = 400u64;
    let read_host = 200u64;
    let read_hardware = 150u64;
    let read_verifier = 100u64;
    let read_liquidity = 50u64;
    let ad_viewer = 30u64;
    let ad_host = 20u64;
    let ad_hardware = 10u64;
    let ad_verifier = 5u64;
    let ad_liquidity = 3u64;
    let ad_miner = 2u64;
    let zero_array = format!("{:?}", [0u8; 32]);

    let block_json = format!(
        r#"{{
            "index": {height},
            "previous_hash": "",
            "timestamp_millis": 0,
            "transactions": [],
            "difficulty": 0,
            "retune_hint": 0,
            "nonce": 0,
            "hash": "b1",
            "coinbase_consumer": 0,
            "coinbase_industrial": 0,
            "storage_sub_ct": 0,
            "read_sub_ct": {read_total},
            "read_sub_viewer_ct": {read_viewer},
            "read_sub_host_ct": {read_host},
            "read_sub_hardware_ct": {read_hardware},
            "read_sub_verifier_ct": {read_verifier},
            "read_sub_liquidity_ct": {read_liquidity},
            "ad_viewer_ct": {ad_viewer},
            "ad_host_ct": {ad_host},
            "ad_hardware_ct": {ad_hardware},
            "ad_verifier_ct": {ad_verifier},
            "ad_liquidity_ct": {ad_liquidity},
            "ad_miner_ct": {ad_miner},
            "compute_sub_ct": 0,
            "proof_rebate_ct": 0,
            "storage_sub_it": 0,
            "read_sub_it": 0,
            "compute_sub_it": 0,
            "read_root": {zero_array},
            "fee_checksum": "",
            "state_root": "",
            "base_fee": 0,
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

    let payouts = explorer
        .block_payouts("b1")
        .expect("payout query")
        .expect("payout breakdown present");
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
    assert_eq!(payouts.advertising.viewer_ct, ad_viewer);
    assert_eq!(payouts.advertising.host_ct, ad_host);
    assert_eq!(payouts.advertising.hardware_ct, ad_hardware);
    assert_eq!(payouts.advertising.verifier_ct, ad_verifier);
    assert_eq!(payouts.advertising.liquidity_ct, ad_liquidity);
    assert_eq!(payouts.advertising.miner_ct, ad_miner);
}

#[test]
fn legacy_block_without_role_totals_uses_json_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");

    let height = 7u64;
    let read_total = 900u64;
    let ad_miner = 45u64;

    // Legacy snapshots only carried the aggregate totals and omitted the per-role
    // payout fields. They also stored partial JSON that fails full Block decoding,
    // so the JSON fallback path must reconstruct the breakdown using the hash hint.
    let legacy_block_json = format!(
        r#"{{
            "index": {height},
            "read_sub_ct": {read_total},
            "ad_miner_ct": {ad_miner}
        }}"#
    );

    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute(
        "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
        params!["legacy-hash", height as i64, legacy_block_json.as_bytes()],
    )
    .expect("insert legacy block");

    let payouts = explorer
        .block_payouts("legacy-hash")
        .expect("payout query")
        .expect("payout breakdown present");

    assert_eq!(payouts.hash, "legacy-hash");
    assert_eq!(payouts.height, height);

    assert_eq!(payouts.read_subsidy.total_ct, read_total);
    assert_eq!(payouts.read_subsidy.viewer_ct, 0);
    assert_eq!(payouts.read_subsidy.host_ct, 0);
    assert_eq!(payouts.read_subsidy.hardware_ct, 0);
    assert_eq!(payouts.read_subsidy.verifier_ct, 0);
    assert_eq!(payouts.read_subsidy.liquidity_ct, 0);
    assert_eq!(payouts.read_subsidy.miner_ct, read_total);

    assert_eq!(payouts.advertising.total_ct, ad_miner);
    assert_eq!(payouts.advertising.viewer_ct, 0);
    assert_eq!(payouts.advertising.host_ct, 0);
    assert_eq!(payouts.advertising.hardware_ct, 0);
    assert_eq!(payouts.advertising.verifier_ct, 0);
    assert_eq!(payouts.advertising.liquidity_ct, 0);
    assert_eq!(payouts.advertising.miner_ct, ad_miner);
}
