#![cfg_attr(not(feature = "dependency-fault"), allow(dead_code))]

#[cfg(not(feature = "dependency-fault"))]
fn main() {
    eprintln!(
        "dependency_fault binary requires the `dependency-fault` feature; rerun with `--features dependency-fault`"
    );
}

#[cfg(feature = "dependency-fault")]
fn main() -> anyhow::Result<()> {
    use std::time::Duration;

    use clap::Parser;
    use std::path::PathBuf;

    use tb_sim::dependency_fault_harness::{
        run_simulation, BackendSelections, CodecBackendChoice, CodingBackendChoice,
        CryptoBackendChoice, FaultSpec, OverlayBackendChoice, RuntimeBackendChoice,
        SimulationRequest, StorageBackendChoice, TransportBackendChoice, OUTPUT_ROOT,
    };

    #[derive(Parser)]
    #[command(
        name = "dependency-fault",
        about = "Simulate dependency faults across wrapper backends"
    )]
    struct Cli {
        #[arg(long, value_enum, default_value_t = RuntimeBackendChoice::Tokio)]
        runtime: RuntimeBackendChoice,
        #[arg(long, value_enum, default_value_t = TransportBackendChoice::Quinn)]
        transport: TransportBackendChoice,
        #[arg(long, value_enum, default_value_t = OverlayBackendChoice::Libp2p)]
        overlay: OverlayBackendChoice,
        #[arg(long, value_enum, default_value_t = StorageBackendChoice::RocksDb)]
        storage: StorageBackendChoice,
        #[arg(long, value_enum, default_value_t = CodingBackendChoice::ReedSolomon)]
        coding: CodingBackendChoice,
        #[arg(long, value_enum, default_value_t = CryptoBackendChoice::Dalek)]
        crypto: CryptoBackendChoice,
        #[arg(long, value_enum, default_value_t = CodecBackendChoice::Bincode)]
        codec: CodecBackendChoice,
        #[arg(long, value_name = "TARGET:KIND")]
        fault: Vec<FaultSpec>,
        #[arg(long, default_value_t = 5)]
        duration_secs: u64,
        #[arg(long, default_value_t = 1)]
        iterations: u32,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        no_logs: bool,
    }

    let cli = Cli::parse();
    let selections = BackendSelections {
        runtime: cli.runtime,
        transport: cli.transport,
        overlay: cli.overlay,
        storage: cli.storage,
        coding: cli.coding,
        crypto: cli.crypto,
        codec: cli.codec,
    };
    let request = SimulationRequest {
        selections,
        faults: cli.fault,
        duration: Duration::from_secs(cli.duration_secs.max(1)),
        iterations: cli.iterations.max(1),
        output_root: cli.output_dir.unwrap_or_else(|| OUTPUT_ROOT.clone()),
        label: cli.label,
        persist_logs: !cli.no_logs,
    };
    let summary = run_simulation(&request)?;
    println!(
        "simulation artifacts stored under {}",
        summary.base_dir.display()
    );
    for report in summary.reports.iter() {
        println!(
            "- {} iteration {} => metrics: {}, summary: {}",
            report.metrics.scenario,
            report.metrics.iteration,
            report.metrics_path.display(),
            report.summary_path.display()
        );
    }
    Ok(())
}
