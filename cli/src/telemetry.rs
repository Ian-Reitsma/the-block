use clap::Subcommand;

#[derive(Subcommand)]
pub enum TelemetryCmd {
    /// Dump current telemetry allocation in bytes
    Dump,
    /// Continuously print telemetry allocation every second
    Tail {
        #[arg(long, default_value_t = 1)]
        interval: u64,
    },
}

pub fn handle(cmd: TelemetryCmd) {
    match cmd {
        TelemetryCmd::Dump => {
            #[cfg(feature = "telemetry")]
            println!("{}", the_block::telemetry::current_alloc_bytes());
            #[cfg(not(feature = "telemetry"))]
            println!("telemetry disabled");
        }
        TelemetryCmd::Tail { interval } => {
            #[cfg(feature = "telemetry")]
            {
                use std::thread::sleep;
                use std::time::Duration;
                loop {
                    println!("{}", the_block::telemetry::current_alloc_bytes());
                    sleep(Duration::from_secs(interval));
                }
            }
            #[cfg(not(feature = "telemetry"))]
            {
                let _ = interval;
                println!("telemetry disabled");
            }
        }
    }
}
