#![deny(warnings)]

use clap::{Parser, Subcommand};
use hex;
use std::fs::File;
use std::io::Write;

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Generate Ed25519 and Dilithium keys in parallel and export keystore
    Gen {
        #[arg(long, default_value = "keystore.json")]
        out: String,
    },
    /// Show available wallet commands
    Help,
    /// List balances for all known tokens
    Balances,
    /// Send tokens to an address with optional ephemeral source
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        ephemeral: bool,
    },
}

pub fn handle(cmd: WalletCmd) {
    match cmd {
        WalletCmd::Gen { out } => {
            #[cfg(feature = "quantum")]
            {
                use std::thread;
                use wallet::pq::generate as pq_generate;
                use wallet::Wallet;
                let ed_handle = thread::spawn(|| Wallet::generate());
                let pq_handle = thread::spawn(|| pq_generate());
                let ed = ed_handle.join().expect("ed25519");
                let (pq_pk, pq_sk) = pq_handle.join().expect("dilithium");
                let mut f = File::create(&out).expect("write");
                let json = serde_json::json!({
                    "ed25519_pub": hex::encode(ed.public_key().to_bytes()),
                    "dilithium_pub": hex::encode(pq_pk.as_bytes()),
                    "dilithium_sk": hex::encode(pq_sk.as_bytes()),
                });
                f.write_all(json.to_string().as_bytes()).expect("write");
                println!("exported keystore to {}", out);
            }
            #[cfg(not(feature = "quantum"))]
            {
                println!("quantum feature not enabled");
            }
        }
        WalletCmd::Help => {
            println!("wallet commands:\n  gen --out <FILE>    Generate key material\n  help                Show this message");
        }
        WalletCmd::Balances => {
            // In a full implementation this would query node RPC.
            println!("token balances:\n  CT: 0\n  IT: 0");
        }
        WalletCmd::Send {
            to,
            amount,
            ephemeral,
        } => {
            if ephemeral {
                let eph = wallet::Wallet::generate();
                println!(
                    "ephemeral address {} used for transfer of {} to {}",
                    hex::encode(eph.public_key().to_bytes()),
                    amount,
                    to
                );
            } else {
                println!("transfer of {} to {} queued", amount, to);
            }
        }
    }
}
