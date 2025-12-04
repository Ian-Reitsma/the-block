#![cfg(feature = "integration-tests")]

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::{fs, path::PathBuf};

use foundation_serialization::json::{Map, Number, Value};
use sys::tempfile::tempdir;
use the_block::{
    gateway::dns::{
        auctions, cancel_sale, clear_ledger_context, complete_sale, install_ledger_context,
        list_for_sale, place_bid, register_stake, stake_snapshot, stake_status, withdraw_stake,
        BlockchainLedger,
    },
    Account, Blockchain, TokenBalance,
};

fn json_map(entries: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn account(address: &str, consumer_balance: u64) -> Account {
    Account {
        address: address.to_string(),
        balance: TokenBalance {
            consumer: consumer_balance,
            industrial: 0,
        },
        nonce: 0,
        pending_consumer: 0,
        pending_industrial: 0,
        pending_nonce: 0,
        pending_nonces: HashSet::new(),
        sessions: Vec::new(),
    }
}

fn install_chain(accounts: Vec<Account>) -> Arc<Mutex<Blockchain>> {
    let chain = Arc::new(Mutex::new(Blockchain::default()));
    {
        let mut guard = chain.lock().unwrap();
        for account in accounts {
            guard.accounts.insert(account.address.clone(), account);
        }
    }
    chain
}

fn history_for(domain: &str) -> Value {
    let params = json_map(vec![("domain", Value::String(domain.to_string()))]);
    auctions(&params).expect("history")["history"].clone()
}

fn extract_records(history: &Value, domain: &str) -> Vec<Value> {
    history
        .as_array()
        .expect("history array")
        .iter()
        .find(|entry| entry["domain"].as_str() == Some(domain))
        .and_then(|entry| entry["records"].as_array().cloned())
        .unwrap_or_else(Vec::new)
}

fn configure_dns_db() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    let root = ROOT.get_or_init(|| {
        let dir = tempdir().expect("tempdir");
        let path = dir.into_path();
        let db_path = path.join("dns");
        fs::create_dir_all(&db_path).expect("create dns db");
        std::env::set_var("TB_DNS_DB_PATH", db_path.to_str().expect("db path as str"));
        path
    });
    let db_path = root.join("dns");
    std::env::set_var("TB_DNS_DB_PATH", db_path.to_str().expect("db path as str"));
    db_path
}

#[testkit::tb_serial]
fn ledger_settlement_updates_balances() {
    configure_dns_db();

    let chain = install_chain(vec![
        account("seller-ledger", 500),
        account("bidder-ledger", 10_000),
        account("treasury", 0),
    ]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    let domain = "ledger-test.block";
    register_stake(&json_map(vec![
        ("reference", Value::String("ledger-stake".to_string())),
        ("owner_account", Value::String("bidder-ledger".to_string())),
        ("deposit_ct", Value::Number(Number::from(1_500))),
    ]))
    .expect("register stake");

    list_for_sale(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("min_bid_ct", Value::Number(Number::from(1_500))),
        ("protocol_fee_bps", Value::Number(Number::from(500))),
        ("royalty_bps", Value::Number(Number::from(100))),
        ("seller_account", Value::String("seller-ledger".to_string())),
    ]))
    .expect("listing");

    place_bid(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("bidder_account", Value::String("bidder-ledger".to_string())),
        ("bid_ct", Value::Number(Number::from(1_500))),
        ("stake_reference", Value::String("ledger-stake".to_string())),
    ]))
    .expect("bid");

    complete_sale(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("force", Value::Bool(true)),
    ]))
    .expect("sale");

    let guard = chain.lock().unwrap();
    assert_eq!(guard.accounts["bidder-ledger"].balance.consumer, 7_000);
    assert_eq!(guard.accounts["seller-ledger"].balance.consumer, 1_910);
    assert_eq!(guard.accounts["treasury"].balance.consumer, 90);
    drop(guard);

    let history = history_for(domain);
    let records = extract_records(&history, domain);
    assert_eq!(records.len(), 1);
    let events = records[0]["ledger_events"]
        .as_array()
        .expect("events array");
    assert_eq!(events.len(), 4);
    for event in events {
        assert!(event["tx_ref"].as_str().expect("tx_ref").starts_with("dns"));
    }

    clear_ledger_context();
}

#[testkit::tb_serial]
fn losing_bidder_keeps_balance_and_unlocked_stake() {
    configure_dns_db();

    let chain = install_chain(vec![
        account("seller-loss", 200),
        account("bidder-low", 5_000),
        account("bidder-high", 5_000),
        account("treasury", 0),
    ]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    let domain = "ledger-loss.block";
    register_stake(&json_map(vec![
        ("reference", Value::String("stake-low".to_string())),
        ("owner_account", Value::String("bidder-low".to_string())),
        ("deposit_ct", Value::Number(Number::from(1_000))),
    ]))
    .expect("register stake low");
    register_stake(&json_map(vec![
        ("reference", Value::String("stake-high".to_string())),
        ("owner_account", Value::String("bidder-high".to_string())),
        ("deposit_ct", Value::Number(Number::from(1_000))),
    ]))
    .expect("register stake high");

    list_for_sale(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("min_bid_ct", Value::Number(Number::from(1_000))),
        ("protocol_fee_bps", Value::Number(Number::from(400))),
        ("royalty_bps", Value::Number(Number::from(100))),
        ("seller_account", Value::String("seller-loss".to_string())),
    ]))
    .expect("listing");

    place_bid(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("bidder_account", Value::String("bidder-low".to_string())),
        ("bid_ct", Value::Number(Number::from(1_100))),
        ("stake_reference", Value::String("stake-low".to_string())),
    ]))
    .expect("initial bid");

    place_bid(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("bidder_account", Value::String("bidder-high".to_string())),
        ("bid_ct", Value::Number(Number::from(1_400))),
        ("stake_reference", Value::String("stake-high".to_string())),
    ]))
    .expect("outbid");

    complete_sale(&json_map(vec![
        ("domain", Value::String(domain.to_string())),
        ("force", Value::Bool(true)),
    ]))
    .expect("sale");

    let guard = chain.lock().unwrap();
    assert_eq!(guard.accounts["bidder-low"].balance.consumer, 4_000);
    assert_eq!(guard.accounts["bidder-high"].balance.consumer, 2_600);
    assert_eq!(guard.accounts["seller-loss"].balance.consumer, 1_530);
    assert_eq!(guard.accounts["treasury"].balance.consumer, 70);
    drop(guard);

    let snapshot = stake_snapshot("stake-low").expect("stake-low snapshot");
    assert_eq!(snapshot.locked_ct, 0);

    let history = history_for(domain);
    let records = extract_records(&history, domain);
    assert_eq!(records.len(), 1);
    let events = records[0]["ledger_events"]
        .as_array()
        .expect("events array");
    assert_eq!(events.len(), 4);

    clear_ledger_context();
}

#[testkit::tb_serial]
fn stake_registration_and_withdrawal_moves_funds() {
    configure_dns_db();

    let chain = install_chain(vec![account("stake-owner", 10_000), account("treasury", 0)]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    register_stake(&json_map(vec![
        ("reference", Value::String("stake-ledger".to_string())),
        ("owner_account", Value::String("stake-owner".to_string())),
        ("deposit_ct", Value::Number(Number::from(2_000))),
    ]))
    .expect("register stake");

    {
        let guard = chain.lock().unwrap();
        assert_eq!(guard.accounts["stake-owner"].balance.consumer, 8_000);
        assert_eq!(guard.accounts["treasury"].balance.consumer, 0);
    }

    withdraw_stake(&json_map(vec![
        ("reference", Value::String("stake-ledger".to_string())),
        ("owner_account", Value::String("stake-owner".to_string())),
        ("withdraw_ct", Value::Number(Number::from(500))),
    ]))
    .expect("withdraw stake");

    {
        let guard = chain.lock().unwrap();
        assert_eq!(guard.accounts["stake-owner"].balance.consumer, 8_500);
    }

    let snapshot = stake_snapshot("stake-ledger").expect("stake snapshot");
    assert_eq!(snapshot.amount_ct, 1_500);
    assert_eq!(snapshot.locked_ct, 0);

    clear_ledger_context();
}

#[testkit::tb_serial]
fn stake_ledger_events_are_persisted() {
    configure_dns_db();

    let chain = install_chain(vec![
        account("stake-ledger-events", 5_000),
        account("treasury", 0),
    ]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    let deposit_response = register_stake(&json_map(vec![
        (
            "reference",
            Value::String("stake-ledger-events".to_string()),
        ),
        (
            "owner_account",
            Value::String("stake-ledger-events".to_string()),
        ),
        ("deposit_ct", Value::Number(Number::from(750))),
    ]))
    .expect("register stake with events");

    let deposit_tx = deposit_response["tx_ref"].as_str().expect("deposit tx ref");
    assert!(deposit_tx.starts_with("dns"));
    let deposit_stake = deposit_response["stake"].as_object().expect("stake object");
    let deposit_events = deposit_stake["ledger_events"]
        .as_array()
        .expect("deposit events");
    assert_eq!(deposit_events.len(), 1);
    assert_eq!(deposit_events[0]["kind"].as_str(), Some("stake_deposit"));
    assert_eq!(deposit_events[0]["amount_ct"].as_u64(), Some(750));

    let withdraw_response = withdraw_stake(&json_map(vec![
        (
            "reference",
            Value::String("stake-ledger-events".to_string()),
        ),
        (
            "owner_account",
            Value::String("stake-ledger-events".to_string()),
        ),
        ("withdraw_ct", Value::Number(Number::from(250))),
    ]))
    .expect("withdraw partial stake");

    let withdraw_stake_view = withdraw_response["stake"]
        .as_object()
        .expect("stake object");
    assert_eq!(
        withdraw_stake_view["amount_ct"].as_u64(),
        Some(500),
        "partial withdrawal updates balance"
    );
    let withdraw_events = withdraw_stake_view["ledger_events"]
        .as_array()
        .expect("withdraw events");
    assert_eq!(withdraw_events.len(), 2);
    assert_eq!(withdraw_events[1]["kind"].as_str(), Some("stake_withdraw"));
    assert_eq!(withdraw_events[1]["amount_ct"].as_u64(), Some(250));

    let withdraw_all_response = withdraw_stake(&json_map(vec![
        (
            "reference",
            Value::String("stake-ledger-events".to_string()),
        ),
        (
            "owner_account",
            Value::String("stake-ledger-events".to_string()),
        ),
        ("withdraw_ct", Value::Number(Number::from(500))),
    ]))
    .expect("withdraw remaining stake");

    let final_stake = withdraw_all_response["stake"]
        .as_object()
        .expect("final stake object");
    assert_eq!(final_stake["amount_ct"].as_u64(), Some(0));
    let final_events = final_stake["ledger_events"]
        .as_array()
        .expect("final events");
    assert_eq!(final_events.len(), 3);
    assert_eq!(final_events[2]["kind"].as_str(), Some("stake_withdraw"));
    assert_eq!(final_events[2]["amount_ct"].as_u64(), Some(500));

    let status = stake_status(&json_map(vec![(
        "reference",
        Value::String("stake-ledger-events".to_string()),
    )]))
    .expect("stake status");
    let status_stake = status["stake"].as_object().expect("status stake object");
    assert_eq!(status_stake["amount_ct"].as_u64(), Some(0));
    let status_events = status_stake["ledger_events"]
        .as_array()
        .expect("status events");
    assert_eq!(status_events.len(), 3);
    assert_eq!(status_events[0]["kind"].as_str(), Some("stake_deposit"));
    assert_eq!(status_events[1]["kind"].as_str(), Some("stake_withdraw"));
    assert_eq!(status_events[2]["kind"].as_str(), Some("stake_withdraw"));

    {
        let guard = chain.lock().unwrap();
        assert_eq!(
            guard.accounts["stake-ledger-events"].balance.consumer,
            5_000
        );
    }

    clear_ledger_context();
}

#[testkit::tb_serial]
fn cancelling_auction_releases_locked_stake() {
    configure_dns_db();

    let chain = install_chain(vec![
        account("seller-cancel", 500),
        account("bidder-cancel", 4_000),
        account("treasury", 0),
    ]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    register_stake(&json_map(vec![
        ("reference", Value::String("stake-cancel".to_string())),
        ("owner_account", Value::String("bidder-cancel".to_string())),
        ("deposit_ct", Value::Number(Number::from(1_200))),
    ]))
    .expect("register bidder stake");

    list_for_sale(&json_map(vec![
        ("domain", Value::String("cancel-me.block".to_string())),
        ("min_bid_ct", Value::Number(Number::from(1_200))),
        ("seller_account", Value::String("seller-cancel".to_string())),
    ]))
    .expect("list domain");

    place_bid(&json_map(vec![
        ("domain", Value::String("cancel-me.block".to_string())),
        ("bidder_account", Value::String("bidder-cancel".to_string())),
        ("bid_ct", Value::Number(Number::from(1_200))),
        ("stake_reference", Value::String("stake-cancel".to_string())),
    ]))
    .expect("bid domain");

    cancel_sale(&json_map(vec![
        ("domain", Value::String("cancel-me.block".to_string())),
        ("seller_account", Value::String("seller-cancel".to_string())),
    ]))
    .expect("cancel sale");

    let snapshot = stake_snapshot("stake-cancel").expect("stake snapshot");
    assert_eq!(snapshot.locked_ct, 0);

    let guard = chain.lock().unwrap();
    assert_eq!(guard.accounts["bidder-cancel"].balance.consumer, 2_800);
    drop(guard);

    let auction_view = auctions(&json_map(vec![(
        "domain",
        Value::String("cancel-me.block".to_string()),
    )]))
    .expect("auction snapshot");
    let auctions_array = auction_view["auctions"].as_array().expect("auctions array");
    assert_eq!(auctions_array.len(), 1);
    let status = auctions_array[0]["status"].as_str().expect("status");
    assert_eq!(status, "cancelled");

    clear_ledger_context();
}

#[testkit::tb_serial]
fn dns_auction_summary_reports_metrics() {
    configure_dns_db();

    let chain = install_chain(vec![
        account("seller-summary", 1_000),
        account("bidder-summary", 5_000),
        account("bidder-active", 5_000),
        account("treasury", 0),
    ]);

    install_ledger_context(Arc::new(BlockchainLedger::new(
        Arc::clone(&chain),
        "treasury".to_string(),
    )));

    register_stake(&json_map(vec![
        ("reference", Value::String("stake-summary".to_string())),
        ("owner_account", Value::String("bidder-summary".to_string())),
        ("deposit_ct", Value::Number(Number::from(2_000))),
    ]))
    .expect("register settled stake");
    register_stake(&json_map(vec![
        ("reference", Value::String("stake-active".to_string())),
        ("owner_account", Value::String("bidder-active".to_string())),
        ("deposit_ct", Value::Number(Number::from(1_200))),
    ]))
    .expect("register active stake");

    let settled_domain = "summary-finished.block";
    list_for_sale(&json_map(vec![
        ("domain", Value::String(settled_domain.to_string())),
        ("min_bid_ct", Value::Number(Number::from(1_500))),
        ("protocol_fee_bps", Value::Number(Number::from(400))),
        ("royalty_bps", Value::Number(Number::from(100))),
        ("seller_account", Value::String("seller-summary".to_string())),
    ]))
    .expect("list settled domain");
    place_bid(&json_map(vec![
        ("domain", Value::String(settled_domain.to_string())),
        ("bidder_account", Value::String("bidder-summary".to_string())),
        ("bid_ct", Value::Number(Number::from(1_700))),
        ("stake_reference", Value::String("stake-summary".to_string())),
    ]))
    .expect("settled bid");
    complete_sale(&json_map(vec![
        ("domain", Value::String(settled_domain.to_string())),
        ("force", Value::Bool(true)),
    ]))
    .expect("settled sale");

    let active_domain = "summary-active.block";
    list_for_sale(&json_map(vec![
        ("domain", Value::String(active_domain.to_string())),
        ("min_bid_ct", Value::Number(Number::from(900))),
        ("protocol_fee_bps", Value::Number(Number::from(300))),
        ("royalty_bps", Value::Number(Number::from(50))),
        ("seller_account", Value::String("seller-summary".to_string())),
    ]))
    .expect("list active domain");
    place_bid(&json_map(vec![
        ("domain", Value::String(active_domain.to_string())),
        ("bidder_account", Value::String("bidder-active".to_string())),
        ("bid_ct", Value::Number(Number::from(1_000))),
        ("stake_reference", Value::String("stake-active".to_string())),
    ]))
    .expect("active bid");

    let snapshot = auctions(&json_map(vec![(
        "metrics_window_secs",
        Value::Number(Number::from(3_600)),
    )]))
    .expect("auction snapshot");
    let summary = snapshot["summary"]
        .as_object()
        .expect("summary object");
    let counts = summary["auction_counts"]
        .as_object()
        .expect("counts map");
    assert!(
        counts["active"].as_u64().unwrap_or(0) >= 1,
        "active auctions reported"
    );
    assert!(
        counts["settled"].as_u64().unwrap_or(0) >= 1,
        "settled auctions reported"
    );
    let stake = summary["stake_snapshot"]
        .as_object()
        .expect("stake snapshot");
    assert!(
        stake["total_locked_ct"].as_u64().unwrap_or(0) >= 1_000,
        "stake snapshot captures locked stake"
    );
    let metrics = summary["metrics"].as_object().expect("metrics map");
    assert!(
        metrics["auction_completions"].as_u64().unwrap_or(0) >= 1,
        "auction completions counted"
    );
    assert!(
        metrics["settlement_stats"].is_object(),
        "settlement stats present"
    );

    clear_ledger_context();
}
