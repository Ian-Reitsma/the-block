use the_block::governance::Params;

fn main() {
    let params = Params::default();
    println!("snapshot interval: {}s", params.snapshot_interval_secs);
}
