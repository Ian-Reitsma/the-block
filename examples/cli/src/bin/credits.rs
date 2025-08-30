use clap::{Parser, Subcommand};
use reqwest::blocking::Client;

#[derive(Parser)]
#[command(name = "credits")] 
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:8545")]
    rpc: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Meter { provider: String },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Meter { provider } => {
            let body = serde_json::json!({
                "method": "credits.meter",
                "params": {"provider": provider},
            });
            let val: serde_json::Value = Client::new()
                .post(&cli.rpc)
                .json(&body)
                .send()
                .expect("rpc")
                .json()
                .expect("json");
            println!("{}", serde_json::to_string_pretty(&val["result"]).unwrap());
        }
    }
}
