use std::fs;
use the_block::compute_market::{
    workloads::inference::BlockTorchInference, Workload, WorkloadRunner,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("slice file");
    let kind = args.next().unwrap_or_else(|| "transcode".into());
    let data = fs::read(file).expect("read slice");
    let workload = match kind.as_str() {
        "inference" => {
            let inference = BlockTorchInference::new(data.clone(), data);
            Workload::Inference(inference)
        }
        "snark" => Workload::Snark(data),
        _ => Workload::Transcode(data),
    };
    let runner = WorkloadRunner::new();
    let out = runtime::block_on(runner.run(0, workload));
    println!("{}", crypto_suite::hex::encode(out));
}
