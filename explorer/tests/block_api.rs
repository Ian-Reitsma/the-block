#![allow(unexpected_cfgs)]

use explorer::Explorer;
use foundation_sqlite::{params, Connection};
use sys::tempfile;
use the_block::{block_binary, Block, TokenAmount};

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
    let ad_total_usd_micros = 125_000u64;
    let ad_settlement_count = 4u64;
    let ad_price = 1_200_000u64;
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
            "coinbase_block": 0,
            "coinbase_industrial": 0,
            "storage_sub": 0,
            "read_sub": {read_total},
            "read_sub_viewer": {read_viewer},
            "read_sub_host": {read_host},
            "read_sub_hardware": {read_hardware},
            "read_sub_verifier": {read_verifier},
            "read_sub_liquidity": {read_liquidity},
            "ad_viewer": {ad_viewer},
            "ad_host": {ad_host},
            "ad_hardware": {ad_hardware},
            "ad_verifier": {ad_verifier},
            "ad_liquidity": {ad_liquidity},
            "ad_miner": {ad_miner},
            "ad_total_usd_micros": {ad_total_usd_micros},
            "ad_settlement_count": {ad_settlement_count},
            "ad_oracle_price_usd_micros": {ad_price},
            "compute_sub": 0,
            "proof_rebate": 0,
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
    assert_eq!(payouts.read_subsidy.total, read_total);
    assert_eq!(payouts.read_subsidy.viewer, read_viewer);
    assert_eq!(payouts.read_subsidy.host, read_host);
    assert_eq!(payouts.read_subsidy.hardware, read_hardware);
    assert_eq!(payouts.read_subsidy.verifier, read_verifier);
    assert_eq!(payouts.read_subsidy.liquidity, read_liquidity);
    assert_eq!(
        payouts.read_subsidy.miner,
        read_total - (read_viewer + read_host + read_hardware + read_verifier + read_liquidity)
    );

    assert_eq!(
        payouts.advertising.total,
        ad_viewer + ad_host + ad_hardware + ad_verifier + ad_liquidity + ad_miner
    );
    assert_eq!(payouts.advertising.viewer, ad_viewer);
    assert_eq!(payouts.advertising.host, ad_host);
    assert_eq!(payouts.advertising.hardware, ad_hardware);
    assert_eq!(payouts.advertising.verifier, ad_verifier);
    assert_eq!(payouts.advertising.liquidity, ad_liquidity);
    assert_eq!(payouts.advertising.miner, ad_miner);
    assert_eq!(payouts.total_usd_micros, ad_total_usd_micros);
    assert_eq!(payouts.settlement_count, ad_settlement_count);
    assert_eq!(payouts.price_usd_micros, ad_price);
}

#[test]
fn block_lookup_handles_zero_advertising() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");

    let height = 22u64;
    let ad_liquidity = 0u64;
    let ad_host = 9u64;
    let ad_viewer = 11u64;
    let ad_hardware = 5u64;
    let ad_verifier = 3u64;
    let ad_miner = 2u64;
    let ad_total_usd_micros = 310_000u64;
    let ad_price = 950_000u64;
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
            "hash": "zero-ad",
            "coinbase_block": 0,
            "coinbase_industrial": 0,
            "storage_sub": 0,
            "read_sub": 0,
            "read_sub_viewer": 0,
            "read_sub_host": 0,
            "read_sub_hardware": 0,
            "read_sub_verifier": 0,
            "read_sub_liquidity": 0,
            "ad_viewer": {ad_viewer},
            "ad_host": {ad_host},
            "ad_hardware": {ad_hardware},
            "ad_verifier": {ad_verifier},
            "ad_liquidity": {ad_liquidity},
            "ad_miner": {ad_miner},
            "ad_total_usd_micros": {ad_total_usd_micros},
            "ad_settlement_count": 2,
            "ad_oracle_price_usd_micros": {ad_price},
            "compute_sub": 0,
            "proof_rebate": 0,
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
        params!["zero-ad", height as i64, block_json.as_bytes()],
    )
    .expect("insert block");

    let payouts = explorer
        .block_payouts("zero-ad")
        .expect("payout query")
        .expect("payout breakdown present");

    assert_eq!(payouts.hash, "zero-ad");
    assert_eq!(payouts.height, height);
    assert_eq!(payouts.advertising.liquidity, 0);
    assert_eq!(payouts.advertising.host, ad_host);
    assert_eq!(
        payouts.advertising.total,
        ad_viewer + ad_host + ad_hardware + ad_verifier + ad_miner
    );
    assert_eq!(payouts.total_usd_micros, ad_total_usd_micros);
    assert_eq!(payouts.price_usd_micros, ad_price);
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
            "read_sub": {read_total},
            "ad_miner": {ad_miner}
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

    assert_eq!(payouts.read_subsidy.total, read_total);
    assert_eq!(payouts.read_subsidy.viewer, 0);
    assert_eq!(payouts.read_subsidy.host, 0);
    assert_eq!(payouts.read_subsidy.hardware, 0);
    assert_eq!(payouts.read_subsidy.verifier, 0);
    assert_eq!(payouts.read_subsidy.liquidity, 0);
    assert_eq!(payouts.read_subsidy.miner, read_total);

    assert_eq!(payouts.advertising.total, ad_miner);
    assert_eq!(payouts.advertising.viewer, 0);
    assert_eq!(payouts.advertising.host, 0);
    assert_eq!(payouts.advertising.hardware, 0);
    assert_eq!(payouts.advertising.verifier, 0);
    assert_eq!(payouts.advertising.liquidity, 0);
    assert_eq!(payouts.advertising.miner, ad_miner);
    assert_eq!(payouts.total_usd_micros, 0);
    assert_eq!(payouts.price_usd_micros, 0);
}

#[test]
fn binary_and_json_snapshots_mix_without_breaking_payouts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("explorer.db");
    let explorer = Explorer::open(&db_path).expect("open explorer");

    let height = 42u64;
    let read_total = 5_000u64;
    let read_viewer = 2_000u64;
    let read_host = 1_000u64;
    let read_hardware = 800u64;
    let read_verifier = 600u64;
    let read_liquidity = 400u64;
    let ad_viewer = 120u64;
    let ad_host = 80u64;
    let ad_hardware = 60u64;
    let ad_verifier = 40u64;
    let ad_liquidity = 30u64;
    let ad_miner = 20u64;
    let ad_total_usd_micros = 2_450_000u64;
    let ad_settlement_count = 9u64;
    let ad_price = 1_050_000u64;

    let block = Block {
        index: height,
        hash: "binary-hash".into(),
        read_sub: TokenAmount::new(read_total),
        read_sub_viewer: TokenAmount::new(read_viewer),
        read_sub_host: TokenAmount::new(read_host),
        read_sub_hardware: TokenAmount::new(read_hardware),
        read_sub_verifier: TokenAmount::new(read_verifier),
        read_sub_liquidity: TokenAmount::new(read_liquidity),
        ad_viewer: TokenAmount::new(ad_viewer),
        ad_host: TokenAmount::new(ad_host),
        ad_hardware: TokenAmount::new(ad_hardware),
        ad_verifier: TokenAmount::new(ad_verifier),
        ad_liquidity: TokenAmount::new(ad_liquidity),
        ad_miner: TokenAmount::new(ad_miner),
        ad_total_usd_micros,
        ad_settlement_count,
        ad_oracle_price_usd_micros: ad_price,
        ..Block::default()
    };

    let block_bytes = block_binary::encode_block(&block).expect("encode block");
    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute(
        "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
        params![block.hash.as_str(), block.index as i64, block_bytes],
    )
    .expect("insert binary block");

    let legacy_json = r#"{
        "index": 99,
        "hash": "json-hash",
        "read_sub": 777,
        "ad_miner": 55
    }"#;
    conn.execute(
        "INSERT OR REPLACE INTO blocks (hash, height, data) VALUES (?1, ?2, ?3)",
        params!["json-hash", 99i64, legacy_json.as_bytes()],
    )
    .expect("insert json block");

    let binary_payouts = explorer
        .block_payouts("binary-hash")
        .expect("binary payout query")
        .expect("binary breakdown present");
    assert_eq!(binary_payouts.hash, "binary-hash");
    assert_eq!(binary_payouts.height, height);
    assert_eq!(binary_payouts.read_subsidy.total, read_total);
    assert_eq!(binary_payouts.read_subsidy.viewer, read_viewer);
    assert_eq!(binary_payouts.read_subsidy.host, read_host);
    assert_eq!(binary_payouts.read_subsidy.hardware, read_hardware);
    assert_eq!(binary_payouts.read_subsidy.verifier, read_verifier);
    assert_eq!(binary_payouts.read_subsidy.liquidity, read_liquidity);
    assert_eq!(
        binary_payouts.read_subsidy.miner,
        read_total - (read_viewer + read_host + read_hardware + read_verifier + read_liquidity)
    );

    assert_eq!(
        binary_payouts.advertising.total,
        ad_viewer + ad_host + ad_hardware + ad_verifier + ad_liquidity + ad_miner
    );
    assert_eq!(binary_payouts.advertising.viewer, ad_viewer);
    assert_eq!(binary_payouts.advertising.host, ad_host);
    assert_eq!(binary_payouts.advertising.hardware, ad_hardware);
    assert_eq!(binary_payouts.advertising.verifier, ad_verifier);
    assert_eq!(binary_payouts.advertising.liquidity, ad_liquidity);
    assert_eq!(binary_payouts.advertising.miner, ad_miner);
    assert_eq!(binary_payouts.total_usd_micros, ad_total_usd_micros);
    assert_eq!(binary_payouts.settlement_count, ad_settlement_count);
    assert_eq!(binary_payouts.price_usd_micros, ad_price);

    let json_payouts = explorer
        .block_payouts("json-hash")
        .expect("json payout query")
        .expect("json breakdown present");
    assert_eq!(json_payouts.hash, "json-hash");
    assert_eq!(json_payouts.height, 99);
    assert_eq!(json_payouts.read_subsidy.total, 777);
    assert_eq!(json_payouts.advertising.miner, 55);
}
