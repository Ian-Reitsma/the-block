use the_block::compute_market::workload::{calibrate_gpu, compute_units};

fn main() {
    let data = b"hello world";
    let units = compute_units(data);
    println!("{} bytes -> {} compute units", data.len(), units);
    let gpu = calibrate_gpu(5000);
    println!("calibrated GPU: {:?}", gpu);
}
