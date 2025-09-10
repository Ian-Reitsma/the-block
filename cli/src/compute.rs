use clap::Subcommand;
use serde_json::json;
use the_block::rpc::client::RpcClient;

#[derive(Subcommand)]
pub enum ComputeCmd {
    /// Cancel an in-flight compute job
    Cancel {
        job_id: String,
        #[arg(long, default_value = "http://localhost:26658")]
        url: String,
    },
}

pub fn handle(cmd: ComputeCmd) {
    match cmd {
        ComputeCmd::Cancel { job_id, url } => {
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
            let params = json!({"job_id": job_id});
            let payload = Payload {
                jsonrpc: "2.0",
                id: 1,
                method: "compute.job_cancel",
                params,
                auth: None,
            };
            match client.call(&url, &payload) {
                Ok(resp) => {
                    if let Ok(text) = resp.text() {
                        println!("{}", text);
                    }
                }
                Err(e) => eprintln!("{e}"),
            }
        }
    }
}
