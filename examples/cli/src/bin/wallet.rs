use clap::{Parser, Subcommand, ValueEnum};
use wallet::{Wallet, WalletSigner};

#[derive(Parser)]
#[command(name = "wallet")]
#[command(about = "Wallet utilities")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Stake CT for a service role
    StakeRole {
        #[arg(value_enum)]
        role: Role,
        amount: u64,
        #[arg(long, help = "32-byte seed in hex")]
        seed: String,
        #[arg(long, help = "withdraw instead of bond")]
        withdraw: bool,
        #[arg(long, default_value = "http://127.0.0.1:8545")]
        url: String,
    },
    /// Query rent-escrow balance for an account
    EscrowBalance {
        account: String,
        #[arg(long, default_value = "http://127.0.0.1:8545")]
        url: String,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Role {
    Gateway,
    Storage,
    Exec,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::StakeRole { role, amount, seed, withdraw, url } => {
            let bytes = hex::decode(seed).expect("seed hex");
            assert!(bytes.len() >= 32, "seed too short");
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            let wallet = Wallet::from_seed(&arr);
            let role_str = format!("{:?}", role).to_lowercase();
            let sig = wallet
                .sign_stake(&role_str, amount, withdraw)
                .expect("sign");
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": if withdraw { "consensus.pos.unbond" } else { "consensus.pos.bond" },
                "params": {
                    "id": wallet.public_key_hex(),
                    "role": role_str,
                    "amount": amount,
                    "sig": hex::encode(sig.to_bytes()),
                }
            });
            let client = reqwest::blocking::Client::new();
            match client.post(&url).json(&body).send() {
                Ok(resp) => match resp.json::<serde_json::Value>() {
                    Ok(v) => println!("{}", v["result"].to_string()),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Command::EscrowBalance { account, url } => {
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "rent.escrow.balance",
                "params": {"id": account},
            });
            let client = reqwest::blocking::Client::new();
            match client.post(&url).json(&payload).send() {
                Ok(resp) => match resp.json::<serde_json::Value>() {
                    Ok(v) => println!("{}", v["result"].as_u64().unwrap_or(0)),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
    }
}

