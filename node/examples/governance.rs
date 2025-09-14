use the_block::governance::params::Params;

fn main() {
    let params = Params::default();
    println!("snapshot interval: {}s", params.snapshot_interval_secs);
}
