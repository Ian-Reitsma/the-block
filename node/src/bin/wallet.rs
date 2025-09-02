use clap::{Parser, Subcommand, ValueEnum};
use hex::{decode, encode};
use reqwest::blocking::Client;
use wallet::{hardware::MockHardwareWallet, Wallet, WalletSigner};

use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

/// Simple CLI for wallet operations.
#[derive(Parser)]
#[command(name = "wallet")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new wallet and print the public key as hex.
    Generate,
    /// Sign a message given a hex-encoded seed and print the signature as hex.
    Sign { seed: String, message: String },
    /// Sign a message using a mock hardware wallet.
    SignHw { message: String },
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
    /// Chunk a file and build a BlobTx, printing the blob root
    BlobPut { file: String, owner: String },
    /// Retrieve a blob by its manifest hash and write to an output file
    BlobGet { blob_id: String, out: String },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Role {
    Gateway,
    Storage,
    Exec,
}

struct DummyProvider {
    id: String,
}

impl Provider for DummyProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Generate => {
            let wallet = Wallet::generate();
            println!("{}", encode(wallet.public_key()));
        }
        Commands::Sign { seed, message } => {
            let seed_bytes = decode(&seed).expect("hex seed");
            assert_eq!(seed_bytes.len(), 32, "seed must be 32 bytes");
            let mut seed_arr = [0u8; 32];
            seed_arr.copy_from_slice(&seed_bytes);
            let wallet = Wallet::from_seed(&seed_arr);
            let sig = wallet.sign(message.as_bytes()).expect("sign");
            println!("{}", encode(sig.to_bytes()));
        }
        Commands::SignHw { message } => {
            let mut hw = MockHardwareWallet::new();
            hw.connect();
            let sig = hw.sign(message.as_bytes()).expect("sign");
            println!("{}", encode(sig.to_bytes()));
        }
        Commands::StakeRole {
            role,
            amount,
            seed,
            withdraw,
            url,
        } => {
            let bytes = decode(&seed).expect("seed hex");
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
                    "sig": encode(sig.to_bytes()),
                }
            });
            let client = Client::new();
            match client.post(&url).json(&body).send() {
                Ok(resp) => match resp.json::<serde_json::Value>() {
                    Ok(v) => println!("{}", v["result"].to_string()),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Commands::EscrowBalance { account, url } => {
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "rent.escrow.balance",
                "params": {"id": account},
            });
            let client = Client::new();
            match client.post(&url).json(&payload).send() {
                Ok(resp) => match resp.json::<serde_json::Value>() {
                    Ok(v) => println!("{}", v["result"].as_u64().unwrap_or(0)),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Commands::BlobPut { file, owner } => {
            let data = std::fs::read(&file).expect("read file");
            let mut pipeline = StoragePipeline::open("blobstore");
            let mut catalog = NodeCatalog::new();
            catalog.register(DummyProvider { id: "local".into() });
            let (_receipt, tx) = pipeline
                .put_object(&data, &owner, &catalog)
                .expect("store blob");
            println!("{}", hex::encode(tx.blob_root));
        }
        Commands::BlobGet { blob_id, out } => {
            let mut arr = [0u8; 32];
            let bytes = decode(&blob_id).expect("blob id hex");
            arr.copy_from_slice(&bytes[..32]);
            let pipeline = StoragePipeline::open("blobstore");
            match pipeline.get_object(&arr) {
                Ok(data) => {
                    std::fs::write(&out, &data).expect("write file");
                    println!("wrote {} bytes", data.len());
                }
                Err(e) => eprintln!("get error: {e}"),
            }
        }
    }
}
