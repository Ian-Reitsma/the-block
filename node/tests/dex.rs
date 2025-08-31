#![allow(clippy::unwrap_used, clippy::expect_used)]
use the_block::dex::{Order, OrderBook, Side, TrustLedger};

#[test]
fn trust_line_transfer() {
    let mut ledger = TrustLedger::default();
    ledger.establish("alice".into(), "bob".into(), 100);
    assert!(ledger.adjust("alice", "bob", 50));
    assert_eq!(ledger.balance("alice", "bob"), 50);
    assert!(!ledger.adjust("alice", "bob", 60)); // exceeds limit
}

#[test]
fn order_matching() {
    let mut book = OrderBook::default();
    let buy = Order {
        id: 0,
        account: "alice".into(),
        side: Side::Buy,
        amount: 10,
        price: 5,
    };
    let sell = Order {
        id: 0,
        account: "bob".into(),
        side: Side::Sell,
        amount: 10,
        price: 5,
    };
    book.place(buy);
    let trades = book.place(sell);
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].2, 10);
}
