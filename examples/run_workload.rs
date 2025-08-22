use std::fs;
use the_block::compute_market::{Workload, WorkloadRunner};

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("slice file");
    let kind = args.next().unwrap_or_else(|| "transcode".into());
    let data = fs::read(file).expect("read slice");
    let workload = match kind.as_str() {
        "inference" => Workload::Inference(data),
        _ => Workload::Transcode(data),
    };
    let runner = WorkloadRunner::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let out = rt.block_on(runner.run(0, workload));
    println!("{}", hex::encode(out));
}
