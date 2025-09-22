use clap::Subcommand;
use storage::{StorageContract, StorageOffer};
use the_block::{
    generate_keypair, rpc,
    transaction::{sign_tx, RawTxPayload},
};

#[derive(Subcommand)]
pub enum StorageCmd {
    /// Upload data to storage market
    Upload {
        object_id: String,
        provider_id: String,
        bytes: u64,
        shares: u16,
        price: u64,
        retention: u64,
    },
    /// Challenge a storage provider
    Challenge {
        object_id: String,
        chunk: u64,
        block: u64,
    },
    /// List provider quotas and recent upload metrics
    Providers {
        #[arg(long)]
        json: bool,
    },
    /// Toggle maintenance mode for a provider
    Maintenance {
        provider_id: String,
        #[arg(long, default_value_t = true)]
        maintenance: bool,
    },
    /// Show recent repair attempts and outcomes
    RepairHistory {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Trigger the repair loop once and print summary statistics
    RepairRun {},
    /// Force a repair attempt for a manifest chunk
    RepairChunk {
        manifest: String,
        chunk: u32,
        #[arg(long)]
        force: bool,
    },
}

pub fn handle(cmd: StorageCmd) {
    match cmd {
        StorageCmd::Upload {
            object_id,
            provider_id,
            bytes,
            shares,
            price,
            retention,
        } => {
            let contract = StorageContract {
                object_id: object_id.clone(),
                provider_id: provider_id.clone(),
                original_bytes: bytes,
                shares,
                price_per_block: price,
                start_block: 0,
                retention_blocks: retention,
                next_payment_block: 1,
                accrued: 0,
            };
            let total = price * retention;
            let payload = RawTxPayload {
                from_: "wallet".into(),
                to: provider_id.clone(),
                amount_consumer: total,
                amount_industrial: 0,
                fee: 0,
                pct_ct: 100,
                nonce: 0,
                memo: Vec::new(),
            };
            let (sk, _pk) = generate_keypair();
            let _signed = sign_tx(&sk, &payload).expect("signing");
            let offer = StorageOffer::new(provider_id, bytes, price, retention);
            let resp = rpc::storage::upload(contract, vec![offer]);
            println!("{}", resp);
            println!("reserved {} CT", total);
        }
        StorageCmd::Challenge {
            object_id,
            chunk,
            block,
        } => {
            use blake3::Hasher;
            let mut h = Hasher::new();
            h.update(object_id.as_bytes());
            h.update(&chunk.to_le_bytes());
            let mut proof = [0u8; 32];
            proof.copy_from_slice(h.finalize().as_bytes());
            let resp = rpc::storage::challenge(&object_id, chunk, proof, block);
            println!("{}", resp);
        }
        StorageCmd::Providers { json } => {
            let resp = rpc::storage::provider_profiles();
            if json {
                println!("{}", resp);
            } else if let Some(list) = resp.get("profiles").and_then(|v| v.as_array()) {
                println!(
                    "{:>20} {:>12} {:>8} {:>10} {:>8} {:>8} {:>6}",
                    "provider", "quota_bytes", "chunk", "throughput", "loss", "rtt_ms", "maint"
                );
                for entry in list {
                    let provider = entry
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let quota = entry
                        .get("quota_bytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let chunk = entry
                        .get("preferred_chunk")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let throughput = entry
                        .get("throughput_bps")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let loss = entry.get("loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let rtt = entry.get("rtt_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let maintenance = entry
                        .get("maintenance")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    println!(
                        "{:>20} {:>12} {:>8} {:>10.0} {:>8.3} {:>8.1} {:>6}",
                        provider,
                        quota,
                        chunk,
                        throughput,
                        loss,
                        rtt,
                        if maintenance { "yes" } else { "no" }
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::Maintenance {
            provider_id,
            maintenance,
        } => {
            let resp = rpc::storage::set_provider_maintenance(&provider_id, maintenance);
            println!("{}", resp);
        }
        StorageCmd::RepairHistory { limit, json } => {
            let resp = rpc::storage::repair_history(limit);
            if json {
                println!("{}", resp);
            } else if let Some(entries) = resp.get("entries").and_then(|v| v.as_array()) {
                println!(
                    "{:<40} {:>8} {:>10} {:>12} {:<}",
                    "manifest", "chunk", "bytes", "status", "error"
                );
                for entry in entries {
                    let manifest = entry
                        .get("manifest")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let chunk = entry
                        .get("chunk")
                        .and_then(|v| v.as_u64())
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "-".into());
                    let bytes = entry.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                    let status = entry.get("status").and_then(|v| v.as_str()).unwrap_or("-");
                    let error = entry.get("error").and_then(|v| v.as_str()).unwrap_or("");
                    println!(
                        "{:<40} {:>8} {:>10} {:>12} {:<}",
                        manifest, chunk, bytes, status, error
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::RepairRun {} => {
            let resp = rpc::storage::repair_run();
            println!("{}", resp);
        }
        StorageCmd::RepairChunk {
            manifest,
            chunk,
            force,
        } => {
            let resp = rpc::storage::repair_chunk(&manifest, chunk, force);
            println!("{}", resp);
        }
    }
}
