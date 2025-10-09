use explorer::Explorer;
use sys::tempfile;
use the_block::{
    compute_market::Job,
    dex::order_book::{Order, OrderBook, Side},
};

#[test]
fn index_and_query_order_book_and_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("explorer.db");
    let ex = Explorer::open(&db).unwrap();

    let mut book = OrderBook::default();
    book.bids.entry(10).or_default().push_back(Order {
        id: 1,
        account: "alice".into(),
        side: Side::Buy,
        amount: 5,
        price: 10,
        max_slippage_bps: 0,
    });
    ex.index_order_book(&book).unwrap();
    let orders = ex.order_book().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].side, "buy");

    let job = Job {
        job_id: "job1".into(),
        buyer: "bob".into(),
        slices: vec![],
        price_per_unit: 1,
        consumer_bond: 0,
        workloads: vec![],
        capability: Default::default(),
        deadline: 0,
        priority: Default::default(),
    };
    ex.index_job(&job, "prov", "pending").unwrap();
    let jobs = ex.compute_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job_id, "job1");
}
