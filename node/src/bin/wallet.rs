use clap::{Parser, Subcommand};
use hex::encode;
use wallet::{hardware::MockHardwareWallet, Wallet, WalletSigner};

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
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Generate => {
            let wallet = Wallet::generate();
            println!("{}", encode(wallet.public_key()));
        }
        Commands::Sign { seed, message } => {
            let seed_bytes = hex::decode(&seed).expect("hex seed");
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
    }
}
