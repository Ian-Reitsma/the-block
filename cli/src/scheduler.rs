use clap::Subcommand;
use serde_json::json;
use the_block::rpc::client::RpcClient;

#[derive(Subcommand)]
pub enum SchedulerCmd {
    /// Show scheduler queue depths and weights
    Stats {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

pub fn handle(cmd: SchedulerCmd) {
    match cmd {
        SchedulerCmd::Stats { url } => {
            let client = RpcClient::from_env();
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                jsonrpc: &'static str,
                id: u32,
                method: &'static str,
                params: serde_json::Value,
                #[serde(skip_serializing_if = "Option::is_none")]
                auth: Option<&'a str>,
            }
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "scheduler.stats",
                params: json!({}),
                auth: None,
            };
            if let Ok(resp) = client.call(&url, &payload) {
                if let Ok(text) = resp.text() {
                    println!("{}", text);
                }
            }
        }
    }
}
