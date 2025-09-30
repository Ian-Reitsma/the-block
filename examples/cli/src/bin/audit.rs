use clap::Parser;
use httpd::{BlockingClient, Method};
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[arg(default_value = "http://127.0.0.1:8545")]
    rpc: String,
}

#[derive(Deserialize, Debug)]
struct Summary {
    epoch: u64,
    receipts: u64,
    invalid: u64,
}

fn main() {
    let cli = Cli::parse();
    let body = serde_json::json!({"method":"settlement.audit"});
    let res: serde_json::Value = BlockingClient::default()
        .request(Method::Post, &cli.rpc)
        .and_then(|builder| builder.json(&body))
        .and_then(|builder| builder.send())
        .expect("rpc")
        .json()
        .expect("json");
    let list: Vec<Summary> = serde_json::from_value(res["result"].clone()).unwrap_or_default();
    for s in list {
        println!("epoch {} receipts {} invalid {}", s.epoch, s.receipts, s.invalid);
    }
}
