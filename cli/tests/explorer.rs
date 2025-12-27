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
    let ad_total_usd_micros = 88_000u64;
    let ad_settlement_count = 5u64;
    let ad_price = 1_250_000u64;
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
            "ad_total_usd_micros": {ad_total_usd_micros},
            "ad_settlement_count": {ad_settlement_count},
            "ad_oracle_price_usd_micros": {ad_price},
            "compute_sub_ct": 0,
            "proof_rebate_ct": 0,
            "read_root": {zeros},
            "fee_checksum": "",
            "state_root": "",
            "base_fee": 0,
            "l2_roots": [],
            "l2_sizes": [],
            "vdf_commit": {zeros},
            "vdf_output": {zeros},
            "vdf_proof": [],
            "treasury_events": [
                {{
                    "disbursement_id": 7,
                    "destination": "treasury-dest",
                    "amount": 12345,
                    "memo": "single token model",
                    "scheduled_epoch": 99,
                    "tx_hash": "0xdeadbeef",
                    "executed_at": 170000
                }}
            ]
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
            format: contract_cli::explorer::PayoutOutputFormat::Json,
        },
        &mut output,
    )
    .expect("block payouts by hash");
    let breakdown_json: json::Value = json::from_slice(&output).expect("json payload");
    let breakdown =
        explorer::BlockPayoutBreakdown::from_json_map(&breakdown_json).expect("payout breakdown");
    assert_eq!(breakdown.hash, "block-42");
    assert_eq!(breakdown.height, height);
    assert_eq!(breakdown.read_subsidy.total, read_total);
    assert_eq!(breakdown.read_subsidy.viewer, read_viewer);
    assert_eq!(breakdown.read_subsidy.host, read_host);
    assert_eq!(breakdown.read_subsidy.miner, 0);
    assert_eq!(
        breakdown.advertising.total,
        ad_viewer + ad_host + ad_hardware + ad_verifier + ad_liquidity + ad_miner
    );
    assert_eq!(breakdown.advertising.viewer, ad_viewer);
    assert_eq!(breakdown.advertising.miner, ad_miner);
    assert_eq!(breakdown.total_usd_micros, ad_total_usd_micros);
    assert_eq!(breakdown.settlement_count, ad_settlement_count);
    assert_eq!(breakdown.price_usd_micros, ad_price);
    assert_eq!(breakdown.treasury_events.len(), 1);
    let timeline = &breakdown.treasury_events[0];
    assert_eq!(timeline.disbursement_id, 7);
    assert_eq!(timeline.destination, "treasury-dest");
    assert_eq!(timeline.amount, 12_345);
    assert_eq!(timeline.memo, "single token model");
    assert_eq!(timeline.scheduled_epoch, 99);
    assert_eq!(timeline.tx_hash, "0xdeadbeef");
    assert_eq!(timeline.executed_at, 170000);

    let mut height_output = Vec::new();
    handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str,
            hash: None,
            height: Some(height),
            format: contract_cli::explorer::PayoutOutputFormat::Json,
        },
        &mut height_output,
    )
    .expect("block payouts by height");
    let height_breakdown_json: json::Value =
        json::from_slice(&height_output).expect("json payload by height");
    let height_breakdown = explorer::BlockPayoutBreakdown::from_json_map(&height_breakdown_json)
        .expect("payout breakdown by height");
    assert_eq!(height_breakdown.read_subsidy.liquidity, read_liquidity);
    assert_eq!(height_breakdown.advertising.liquidity, ad_liquidity);
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
            format: contract_cli::explorer::PayoutOutputFormat::Json,
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
            format: contract_cli::explorer::PayoutOutputFormat::Json,
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
            format: contract_cli::explorer::PayoutOutputFormat::Json,
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
            format: contract_cli::explorer::PayoutOutputFormat::Json,
        },
        &mut none_sink,
    )
    .expect_err("supplying neither hash nor height should error");
    assert_eq!(err, "must supply exactly one of '--hash' or '--height'");
}

#[test]
fn block_payouts_supports_table_and_prometheus_formats() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("explorer.db");
    let _store = ExplorerStore::open(&db_path).expect("open explorer");
    let conn = Connection::open(&db_path).expect("open sqlite");
    let block_json = r#"{
        "index": 7,
        "previous_hash": "prev",
        "timestamp_millis": 123,
        "transactions": [],
        "difficulty": 0,
        "retune_hint": 0,
        "nonce": 0,
        "hash": "block-7",
        "coinbase_block": 0,
        "coinbase_industrial": 0,
        "storage_sub_ct": 0,
        "read_sub_ct": 600,
        "read_sub_viewer_ct": 200,
        "read_sub_host_ct": 150,
        "read_sub_hardware_ct": 100,
        "read_sub_verifier_ct": 50,
        "read_sub_liquidity_ct": 75,
        "ad_viewer": 90,
        "ad_host": 60,
        "ad_hardware": 45,
        "ad_verifier": 30,
        "ad_liquidity": 15,
        "ad_miner": 12,
        "ad_total_usd_micros": 64000,
        "ad_settlement_count": 4,
        "ad_oracle_price_usd_micros": 1100000,
        "compute_sub_ct": 0,
        "proof_rebate_ct": 0,
        "read_root": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "fee_checksum": "",
        "state_root": "",
        "base_fee": 0,
        "l2_roots": [],
        "l2_sizes": [],
        "vdf_commit": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "vdf_output": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "vdf_proof": []
    }"#;
    conn.execute(
        "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
        params!["block-7", 7i64, block_json.as_bytes()],
    )
    .expect("insert block");

    let db_str = db_path.to_string_lossy().into_owned();
    let mut table_output = Vec::new();
    handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str.clone(),
            hash: Some("block-7".into()),
            height: None,
            format: contract_cli::explorer::PayoutOutputFormat::Table,
        },
        &mut table_output,
    )
    .expect("table output");
    let rendered_table = String::from_utf8(table_output).expect("utf8 table");
    assert!(rendered_table.contains("block hash: block-7 (height 7)"));
    assert!(rendered_table.contains("viewer"));
    assert!(rendered_table.contains("600"));
    assert!(rendered_table.contains("252"));
    assert!(rendered_table.contains("ad_total_usd_micros: 64000"));

    let mut prom_output = Vec::new();
    handle_with_writer(
        ExplorerCmd::BlockPayouts {
            db: db_str,
            hash: Some("block-7".into()),
            height: None,
            format: contract_cli::explorer::PayoutOutputFormat::Prom,
        },
        &mut prom_output,
    )
    .expect("prom output");
    let rendered_prom = String::from_utf8(prom_output).expect("utf8 prom");
    assert!(rendered_prom.contains("explorer_block_payout_read_total{role=\"viewer\"} 200"));
    assert!(rendered_prom.contains("explorer_block_payout_ad_total{role=\"miner\"} 12"));
    assert!(rendered_prom.contains("explorer_block_payout_ad_usd_total 64000"));
}
