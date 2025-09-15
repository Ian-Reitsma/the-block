use clap::Subcommand;
use the_block::rpc::client::RpcClient;

#[derive(Subcommand)]
pub enum LightClientCmd {
    /// Show current proof rebate balance
    RebateStatus {
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

pub fn handle(cmd: LightClientCmd) {
    match cmd {
        LightClientCmd::RebateStatus { url } => {
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
                method: "light_client.rebate_status",
                params: serde_json::json!({}),
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
