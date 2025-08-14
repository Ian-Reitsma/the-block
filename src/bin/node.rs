use clap::Parser;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
#[cfg(feature = "telemetry")]
use the_block::serve_metrics;
use the_block::{rpc::spawn_rpc_server, spawn_purge_loop_thread, Blockchain, ShutdownFlag};

#[derive(Parser)]
#[command(author, version, about = "Run a basic node with JSON-RPC controls")]
struct Opts {
    /// Address to bind the JSON-RPC server to
    #[arg(long, default_value = "127.0.0.1:3030")]
    rpc_addr: String,

    /// Seconds between mempool purge sweeps (0 to disable)
    #[arg(long, default_value_t = 0)]
    mempool_purge_interval: u64,

    /// Optional address to expose Prometheus metrics on
    #[arg(long)]
    serve_metrics: Option<String>,

    /// Directory for chain data
    #[arg(long, default_value = "node-data")]
    data_dir: String,
}

fn main() {
    let opts = Opts::parse();
    let bc = Arc::new(Mutex::new(Blockchain::new(&opts.data_dir)));

    if let Some(_addr) = &opts.serve_metrics {
        #[cfg(feature = "telemetry")]
        let _ = serve_metrics(_addr);
        #[cfg(not(feature = "telemetry"))]
        eprintln!("telemetry feature not enabled");
    }

    if opts.mempool_purge_interval > 0 {
        let flag = ShutdownFlag::new();
        spawn_purge_loop_thread(Arc::clone(&bc), opts.mempool_purge_interval, flag.as_arc());
    }

    let mining = Arc::new(AtomicBool::new(false));
    // `spawn_rpc_server` now surfaces JSON-RPC compliant errors for malformed
    // requests or unknown methods.
    let (rpc_addr, handle) = spawn_rpc_server(Arc::clone(&bc), Arc::clone(&mining), &opts.rpc_addr)
        .expect("spawn rpc server");
    println!("RPC listening on {rpc_addr}");
    let _ = handle.join();
}
