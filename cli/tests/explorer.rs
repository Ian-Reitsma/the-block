use contract_cli::explorer::{handle_with_writer, ExplorerCmd};
use explorer::Explorer as ExplorerStore;
use foundation_serialization::json;
use foundation_sqlite::{params, Connection};
use sys::tempfile::tempdir;

#[test]
fn block_payouts_command_prints_breakdown_for_hash_and_height() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let _store = ExplorerStore::open(&db_path).expect("open explorer");

    let height = 42u64;
    let read_total = 450u64;
    let read_viewer = 200u64;
    let read_host = 100u64;
    let read_hardware = 50u64;
    let read_verifier = 50u64;
    let read_liquidity = 55u64;
    let ad_viewer = 30u64;
    let ad_host = 20u64;
    let ad_hardware = 10u64;
    let ad_verifier = 5u64;
    let ad_liquidity = 5u64;
    let ad_miner = 10u64;
    let zeros = format!("{:?}", [0u8; 32]);

    let block_json = format!(
        r#"{{
            "index": {height},
            "previous_hash": "prev",
            "timestamp_millis": 1234,
            "transactions": [],
            "difficulty": 0,
            "retune_hint": 0,
            "nonce": 0,
            "hash": "block-42",
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
            "read_root": {zeros},
            "fee_checksum": "",
            "state_root": "",
            "base_fee": 0,
            "l2_roots": [],
            "l2_sizes": [],
            "vdf_commit": {zeros},
            "vdf_output": {zeros},
            "vdf_proof": []
        }}"#
    );

    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute(
        "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
        params!["block-42", height as i64, block_json.as_bytes()],
    )
    .expect("insert block");

    let db_str = db_path.to_string_lossy().into_owned();

    let mut output = Vec::new();
    handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str.clone(),
            hash: Some("block-42".into()),
            height: None,
        },
        &mut output,
    )
    .expect("block payouts by hash");
    let breakdown_json: json::Value = json::from_slice(&output).expect("json payload");
    let breakdown =
        explorer::BlockPayoutBreakdown::from_json_map(&breakdown_json).expect("payout breakdown");
    assert_eq!(breakdown.hash, "block-42");
    assert_eq!(breakdown.height, height);
    assert_eq!(breakdown.read_subsidy.total_ct, read_total);
    assert_eq!(breakdown.read_subsidy.viewer_ct, read_viewer);
    assert_eq!(breakdown.read_subsidy.host_ct, read_host);
    assert_eq!(breakdown.read_subsidy.miner_ct, 0);
    assert_eq!(
        breakdown.advertising.total_ct,
        ad_viewer + ad_host + ad_hardware + ad_verifier + ad_liquidity + ad_miner
    );
    assert_eq!(breakdown.advertising.viewer_ct, ad_viewer);
    assert_eq!(breakdown.advertising.miner_ct, ad_miner);

    let mut height_output = Vec::new();
    handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str,
            hash: None,
            height: Some(height),
        },
        &mut height_output,
    )
    .expect("block payouts by height");
    let height_breakdown_json: json::Value =
        json::from_slice(&height_output).expect("json payload by height");
    let height_breakdown = explorer::BlockPayoutBreakdown::from_json_map(&height_breakdown_json)
        .expect("payout breakdown by height");
    assert_eq!(height_breakdown.read_subsidy.liquidity_ct, read_liquidity);
    assert_eq!(height_breakdown.advertising.liquidity_ct, ad_liquidity);
}

#[test]
fn block_payouts_command_errors_when_block_missing() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let _store = ExplorerStore::open(&db_path).expect("open explorer");
    let db_str = db_path.to_string_lossy().into_owned();

    let mut sink = Vec::new();
    let err = handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str.clone(),
            hash: Some("missing-block".into()),
            height: None,
        },
        &mut sink,
    )
    .expect_err("missing block should error");
    assert_eq!(err, "no block found with hash missing-block");

    let mut height_sink = Vec::new();
    let err = handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str,
            hash: None,
            height: Some(99),
        },
        &mut height_sink,
    )
    .expect_err("missing height should error");
    assert_eq!(err, "no block found at height 99");
}

#[test]
fn block_payouts_command_requires_exactly_one_identifier() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let _store = ExplorerStore::open(&db_path).expect("open explorer");
    let db_str = db_path.to_string_lossy().into_owned();

    let mut sink = Vec::new();
    let err = handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str.clone(),
            hash: Some("block-1".into()),
            height: Some(1),
        },
        &mut sink,
    )
    .expect_err("supplying both hash and height should error");
    assert_eq!(err, "must supply exactly one of '--hash' or '--height'");

    let mut none_sink = Vec::new();
    let err = handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str,
            hash: None,
            height: None,
        },
        &mut none_sink,
    )
    .expect_err("supplying neither hash nor height should error");
    assert_eq!(err, "must supply exactly one of '--hash' or '--height'");
}
