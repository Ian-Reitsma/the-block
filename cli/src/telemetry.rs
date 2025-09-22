use crate::rpc::RpcClient;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum TelemetryCmd {
    /// Dump current telemetry allocation in bytes
    Dump,
    /// Continuously print telemetry allocation every second
    Tail {
        #[arg(long, default_value_t = 1)]
        interval: u64,
    },
    /// Configure telemetry sampling and compaction intervals
    Configure {
        #[arg(long)]
        sample_rate: Option<f64>,
        #[arg(long)]
        compaction: Option<u64>,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
        #[arg(long)]
        token: Option<String>,
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
        TelemetryCmd::Configure {
            sample_rate,
            compaction,
            url,
            token,
        } => {
            if sample_rate.is_none() && compaction.is_none() {
                eprintln!("no parameters provided");
                return;
            }
            let client = RpcClient::from_env();
            let payload = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "telemetry.configure",
                "params": {
                    "sample_rate": sample_rate,
                    "compaction_secs": compaction,
                },
            });
            let auth = token.as_ref().map(|t| format!("Bearer {}", t));
            match client.call_with_auth(&url, &payload, auth.as_deref()) {
                Ok(resp) => match resp.text() {
                    Ok(body) => println!("{}", body),
                    Err(err) => eprintln!("telemetry.configure response error: {err}"),
                },
                Err(err) => eprintln!("telemetry.configure failed: {err}"),
            }
        }
    }
}
