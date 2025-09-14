use dkg::run_dkg;

pub fn run_latency_sim() {
    let start = std::time::Instant::now();
    let _ = run_dkg(5, 3);
    let _elapsed = start.elapsed();
    // results would be fed into model evaluation
}
