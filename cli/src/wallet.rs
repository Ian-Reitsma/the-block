use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::Write;

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Generate Ed25519 and Dilithium keys in parallel and export keystore
    Gen { #[arg(long, default_value = "keystore.json")] out: String },
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
    }
}
