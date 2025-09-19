#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use the_block::dex::{storage::EscrowState, Order, OrderBook, Side, TrustLedger};

#[test]
fn trust_line_transfer() {
    let mut ledger = TrustLedger::default();
    ledger.establish("alice".into(), "bob".into(), 100);
    ledger.authorize("alice", "bob");
    assert!(ledger.adjust("alice", "bob", 50));
    assert_eq!(ledger.balance("alice", "bob"), 50);
    assert!(!ledger.adjust("alice", "bob", 60)); // exceeds limit
}

#[test]
fn order_matching() {
    let mut book = OrderBook::default();
    let mut ledger = TrustLedger::default();
    ledger.establish("alice".into(), "bob".into(), 1_000);
    ledger.authorize("alice", "bob");
    ledger.authorize("bob", "alice");
    let buy = Order {
        id: 0,
        account: "alice".into(),
        side: Side::Buy,
        amount: 10,
        price: 5,
        max_slippage_bps: 0,
    };
    let sell = Order {
        id: 0,
        account: "bob".into(),
        side: Side::Sell,
        amount: 10,
        price: 5,
        max_slippage_bps: 0,
    };
    book.place(buy).unwrap();
    let mut esc_state = EscrowState::default();
    let trades = book.place_and_lock(sell, &mut esc_state).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].2, 10);
    // Funds locked in escrow; ledger unchanged until release
    assert_eq!(ledger.balance("alice", "bob"), 0);
    let eid = *esc_state.locks.keys().next().unwrap();
    esc_state.escrow.release(eid, 50).unwrap();
    ledger.adjust("alice", "bob", 50);
    ledger.adjust("bob", "alice", -50);
    assert_eq!(ledger.balance("alice", "bob"), 50);
}

#[test]
fn path_finding() {
    let mut ledger = TrustLedger::default();
    ledger.establish("alice".into(), "bob".into(), 100);
    ledger.establish("bob".into(), "carol".into(), 100);
    ledger.authorize("alice", "bob");
    ledger.authorize("bob", "carol");
    let path = ledger.find_path("alice", "carol", 50).unwrap();
    assert_eq!(path, vec!["alice", "bob", "carol"]);
    // Fails when not authorized
    ledger.establish("carol".into(), "dave".into(), 100);
    assert!(ledger.find_path("alice", "dave", 10).is_none());
}

#[test]
fn slippage_rejects_unfavorable_price() {
    let mut book = OrderBook::default();
    let buy = Order {
        id: 0,
        account: "alice".into(),
        side: Side::Buy,
        amount: 10,
        price: 10,
        max_slippage_bps: 100, // 1%
    };
    let sell = Order {
        id: 0,
        account: "bob".into(),
        side: Side::Sell,
        amount: 10,
        price: 11,
        max_slippage_bps: 0,
    };
    book.place(sell).unwrap();
    assert!(book.place(buy).is_err());
}
