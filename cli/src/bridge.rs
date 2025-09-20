use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, RelayerBundle, RelayerProof,
};
use clap::Subcommand;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum BridgeCmd {
    /// Deposit with light-client proof
    Deposit {
        user: String,
        amount: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        relayers: Vec<String>,
        #[arg(long, default_value = "header.json")]
        header: String,
        #[arg(long, default_value = "proof.json")]
        proof: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
    /// Withdraw using relayer bundle
    Withdraw {
        user: String,
        amount: u64,
        #[arg(long, value_delimiter = ',', required = true)]
        relayers: Vec<String>,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
    /// Challenge a pending withdrawal commitment
    Challenge {
        commitment: String,
        #[arg(long, default_value = "bridge.bin")]
        state: String,
    },
}

fn load_state(path: &PathBuf) -> Bridge {
    if path.exists() {
        let bytes = fs::read(path).expect("read bridge state");
        bincode::deserialize(&bytes).unwrap_or_default()
    } else {
        Bridge::default()
    }
}

fn save_state(path: &PathBuf, bridge: &Bridge) {
    let bytes = bincode::serialize(bridge).expect("serialize bridge state");
    fs::write(path, bytes).expect("write bridge state");
}

fn make_bundle(user: &str, amount: u64, relayers: &[String]) -> RelayerBundle {
    let proofs = relayers
        .iter()
        .map(|id| RelayerProof::new(id, user, amount))
        .collect();
    RelayerBundle::new(proofs)
}

pub fn handle(action: BridgeCmd) {
    match action {
        BridgeCmd::Deposit {
            user,
            amount,
            relayers,
            header,
            proof,
            state,
        } => {
            if relayers.is_empty() {
                eprintln!("at least one relayer must be provided");
                return;
            }
            let path = PathBuf::from(&state);
            let mut bridge = load_state(&path);
            let header_str = fs::read_to_string(&header).expect("read header");
            let pow_header: PowHeader = serde_json::from_str(&header_str).expect("parse header");
            let proof_str = fs::read_to_string(&proof).expect("read proof");
            let proof: Proof = serde_json::from_str(&proof_str).expect("parse proof");
            let mut relayer_set = RelayerSet::default();
            let bundle = make_bundle(&user, amount, &relayers);
            let primary = relayers.first().cloned().unwrap();
            if bridge.deposit_with_relayer(
                &mut relayer_set,
                &primary,
                &user,
                amount,
                &pow_header,
                &proof,
                &bundle,
            ) {
                save_state(&path, &bridge);
                let dir = PathBuf::from("state/bridge_headers");
                fs::create_dir_all(&dir).expect("make header dir");
                let header_view = Header {
                    chain_id: pow_header.chain_id.clone(),
                    height: pow_header.height,
                    merkle_root: pow_header.merkle_root,
                    signature: pow_header.signature,
                };
                let record = serde_json::to_string(&serde_json::json!({
                    "pow_header": &pow_header,
                    "light_header": &header_view,
                    "proof": &proof
                }))
                .expect("encode record");
                let name = hex::encode(header_hash(&header_view));
                fs::write(dir.join(name), record).expect("store header");
                println!("locked");
            } else {
                eprintln!("invalid proof bundle");
            }
        }
        BridgeCmd::Withdraw {
            user,
            amount,
            relayers,
            state,
        } => {
            if relayers.is_empty() {
                eprintln!("at least one relayer must be provided");
                return;
            }
            let path = PathBuf::from(&state);
            let mut bridge = load_state(&path);
            let mut relayer_set = RelayerSet::default();
            let bundle = make_bundle(&user, amount, &relayers);
            let primary = relayers.first().cloned().unwrap();
            if bridge.unlock_with_relayer(&mut relayer_set, &primary, &user, amount, &bundle) {
                let commitment = bundle.aggregate_commitment(&user, amount);
                save_state(&path, &bridge);
                println!("withdrawal pending: {}", hex::encode(commitment));
            } else {
                eprintln!("withdrawal request rejected");
            }
        }
        BridgeCmd::Challenge { commitment, state } => {
            let path = PathBuf::from(&state);
            let mut bridge = load_state(&path);
            let mut relayer_set = RelayerSet::default();
            if let Ok(bytes) = hex::decode(&commitment) {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    if bridge.challenge_withdrawal(&mut relayer_set, key) {
                        save_state(&path, &bridge);
                        println!("challenge recorded");
                    } else {
                        eprintln!("no matching pending withdrawal or already challenged");
                    }
                    return;
                }
            }
            eprintln!("invalid commitment hex");
        }
    }
}
