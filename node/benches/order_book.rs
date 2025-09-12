use criterion::{criterion_group, criterion_main, Criterion};
use the_block::dex::order_book::{Order, OrderBook, Side};

fn bench_order_book(c: &mut Criterion) {
    c.bench_function("place_orders", |b| {
        b.iter(|| {
            let mut book = OrderBook::default();
            for i in 0..100u64 {
                let order = Order {
                    id: 0,
                    account: "alice".to_string(),
                    side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
                    amount: 1,
                    price: i,
                    max_slippage_bps: 0,
                };
                let _ = book.place(order);
            }
        })
    });
}

criterion_group!(benches, bench_order_book);
criterion_main!(benches);
