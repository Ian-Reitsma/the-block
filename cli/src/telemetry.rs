use clap::Subcommand;

#[derive(Subcommand)]
pub enum TelemetryCmd {
    /// Dump current telemetry allocation in bytes
    Dump,
}

pub fn handle(cmd: TelemetryCmd) {
    match cmd {
        TelemetryCmd::Dump => {
            #[cfg(feature = "telemetry")]
            println!("{}", the_block::telemetry::current_alloc_bytes());
            #[cfg(not(feature = "telemetry"))]
            println!("telemetry disabled");
        }
    }
}
