#![cfg(feature = "integration-tests")]
use dex::cfmm::swap_del_pino;
use std::thread;
use the_block::compute_market::price_bands;

#[test]
fn concurrent_dex_and_compute_market() {
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(thread::spawn(|| {
            for _ in 0..100 {
                let _ = swap_del_pino(1000.0, 1000.0, 1.0);
                let _ = price_bands(&[1, 2, 3, 4]);
            }
        }));
    }
    for h in handles {
        h.join().expect("thread");
    }
}
