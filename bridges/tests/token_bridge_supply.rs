use bridges::token_bridge::TokenBridge;
use ledger::Emission;

#[test]
fn supply_bookkeeping_updates_on_lock_and_withdrawal() {
    let mut bridge = TokenBridge::new();

    bridge.lock("btc", 150);
    assert_eq!(bridge.locked_supply("btc"), 150);
    assert_eq!(bridge.minted_supply("btc"), 0);

    bridge.unlock("btc", 40);
    assert_eq!(bridge.locked_supply("btc"), 110);

    bridge.mint("btc", 40);
    assert_eq!(bridge.minted_supply("btc"), 40);

    bridge.burn("btc", 10);
    assert_eq!(bridge.minted_supply("btc"), 30);

    let snapshot = bridge.asset_snapshots();
    assert_eq!(snapshot.len(), 1);
    let asset = &snapshot[0];
    assert_eq!(asset.symbol, "btc");
    assert_eq!(asset.locked, 110);
    assert_eq!(asset.minted, 30);
    match asset.emission {
        Emission::Fixed(amount) => assert_eq!(amount, 0),
        _ => panic!("unexpected emission"),
    }
}

#[test]
fn asset_snapshots_include_unregistered_tokens_after_activity() {
    let mut bridge = TokenBridge::new();
    bridge.mint("eth", 25);
    bridge.lock("eth", 10);

    let mut symbols: Vec<_> = bridge.asset_symbols();
    symbols.sort();
    assert_eq!(symbols, vec!["eth".to_string()]);

    let snapshot = bridge.asset_snapshots();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].symbol, "eth");
    assert_eq!(snapshot[0].locked, 10);
    assert_eq!(snapshot[0].minted, 25);
}
