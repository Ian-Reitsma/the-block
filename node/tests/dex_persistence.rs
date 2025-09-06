use serial_test::serial;
use std::fs;
use std::sync::Once;
use the_block::dex::{escrow::Escrow, DexStore, Order, OrderBook, Side, TrustLedger};

mod util;
use util::temp::temp_dir;

static PY_INIT: Once = Once::new();
fn init() {
    let _ = fs::remove_dir_all("chain_db");
    PY_INIT.call_once(|| {
        pyo3::prepare_freethreaded_python();
    });
}

#[test]
#[serial]
fn order_book_persists() {
    init();
    let dir = temp_dir("dex_store");
    let mut store = DexStore::open(dir.path().to_str().unwrap());
    let mut book = OrderBook::default();
    let mut ledger = TrustLedger::default();
    ledger.establish("alice".into(), "bob".into(), 100);
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
    let mut escrow = Escrow::default();
    book
        .place_settle_persist(buy, &mut ledger, Some(&mut store), &mut escrow)
        .unwrap();
    book
        .place_settle_persist(sell, &mut ledger, Some(&mut store), &mut escrow)
        .unwrap();
    assert_eq!(store.trades().len(), 1);
    drop(book);
    drop(store);
    let store2 = DexStore::open(dir.path().to_str().unwrap());
    assert_eq!(store2.trades().len(), 1);
}
