use rand::Rng;
use std::time::Instant;

/// Rough simulation comparing SNARK verification cost vs. trustless execution.
fn main() {
    let mut rng = rand::thread_rng();
    let samples = 100u64;
    let mut snark_ms = 0u128;
    let mut plain_ms = 0u128;
    for _ in 0..samples {
        let data: Vec<u8> = (0..1024).map(|_| rng.gen::<u32>() as u8).collect();
        let start = Instant::now();
        // pretend to verify SNARK
        let proof = the_block::compute_market::snark::prove(&data, &data).unwrap();
        let _ = the_block::compute_market::snark::verify(&proof, &data, &data).unwrap();
        snark_ms += start.elapsed().as_millis();
        let start = Instant::now();
        let _ = the_block::compute_market::workloads::snark::run(&data);
        plain_ms += start.elapsed().as_millis();
    }
    println!("snark_ms={} plain_ms={}", snark_ms, plain_ms);
}
