use clap::Subcommand;
use std::fs;
use std::path::PathBuf;

use bridges::{
    light_client::{header_hash, Header, Proof},
    Bridge, RelayerProof,
};

#[derive(Subcommand)]
pub enum BridgeCmd {
    /// Deposit with light-client proof
    Deposit {
        user: String,
        amount: u64,
        #[arg(long, default_value = "header.json")]
        header: String,
        #[arg(long, default_value = "proof.json")]
        proof: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
    /// Withdraw using relayer proof
    Withdraw {
        user: String,
        amount: u64,
        relayer: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
}

pub fn handle(action: BridgeCmd) {
    match action {
        BridgeCmd::Deposit {
            user,
            amount,
            header,
            proof,
            state,
        } => {
            let path = PathBuf::from(&state);
            let mut bridge = if path.exists() {
                let bytes = fs::read(&path).expect("read bridge state");
                bincode::deserialize(&bytes).unwrap_or_default()
            } else {
                Bridge::default()
            };
            let header_str = fs::read_to_string(&header).expect("read header");
            let header: Header = serde_json::from_str(&header_str).expect("parse header");
            let proof_str = fs::read_to_string(&proof).expect("read proof");
            let proof: Proof = serde_json::from_str(&proof_str).expect("parse proof");
            if bridge.deposit_verified(&user, amount, &header, &proof) {
                let bytes = bincode::serialize(&bridge).expect("serialize");
                fs::write(&path, bytes).expect("write bridge state");
                let dir = PathBuf::from("state/bridge_headers");
                fs::create_dir_all(&dir).expect("make header dir");
                let record = serde_json::to_string(&serde_json::json!({
                    "header": &header,
                    "proof": &proof
                }))
                .expect("encode record");
                let name = hex::encode(header_hash(&header));
                fs::write(dir.join(name), record).expect("store header");
                println!("locked");
            } else {
                eprintln!("invalid proof");
            }
        }
        BridgeCmd::Withdraw {
            user,
            amount,
            relayer,
            state,
        } => {
            let path = PathBuf::from(&state);
            let mut bridge = if path.exists() {
                let bytes = fs::read(&path).expect("read bridge state");
                bincode::deserialize(&bytes).unwrap_or_default()
            } else {
                Bridge::default()
            };
            let proof = RelayerProof::new(&relayer, &user, amount);
            if bridge.unlock(&user, amount, &proof) {
                let bytes = bincode::serialize(&bridge).expect("serialize");
                fs::write(&path, bytes).expect("write bridge state");
                println!("unlocked");
            } else {
                eprintln!("invalid proof or balance");
            }
        }
    }
}
