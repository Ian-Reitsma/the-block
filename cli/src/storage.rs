use clap::Subcommand;
use storage::{StorageContract, StorageOffer};
use the_block::{
    generate_keypair, rpc,
    simple_db::EngineKind,
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
    /// List stored manifests and active coding algorithms
    Manifests {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
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
                if let Some(engine) = resp.get("engine").and_then(|v| v.as_object()) {
                    let pipeline = engine
                        .get("pipeline")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let rent = engine
                        .get("rent_escrow")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    println!("storage pipeline engine: {pipeline} (rent escrow: {rent})");
                    let recommended = EngineKind::default_for_build().label();
                    if pipeline != recommended || rent != recommended {
                        println!(
                            "warning: recommended storage engine is {recommended}; consider migrating via tools/storage_migrate"
                        );
                    }
                    if engine
                        .get("legacy_mode")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        println!(
                            "warning: storage legacy mode is enabled and will be removed in the next release"
                        );
                    }
                    println!();
                }
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
        StorageCmd::Manifests { limit, json } => {
            let resp = rpc::storage::manifest_summaries(limit);
            if json {
                println!("{}", resp);
            } else if let Some(entries) = resp.get("manifests").and_then(|v| v.as_array()) {
                if let Some(policy) = resp.get("policy").and_then(|v| v.as_object()) {
                    if let Some(erasure) = policy.get("erasure").and_then(|v| v.as_object()) {
                        let algorithm = erasure
                            .get("algorithm")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let fallback = erasure
                            .get("fallback")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let emergency = erasure
                            .get("emergency")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        println!(
                            "erasure policy: {algorithm} (fallback={}, emergency={})",
                            fallback, emergency
                        );
                    }
                    if let Some(compression) = policy.get("compression").and_then(|v| v.as_object())
                    {
                        let algorithm = compression
                            .get("algorithm")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let fallback = compression
                            .get("fallback")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let emergency = compression
                            .get("emergency")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        println!(
                            "compression policy: {algorithm} (fallback={}, emergency={})",
                            fallback, emergency
                        );
                    }
                    println!();
                }
                println!(
                    "{:<64} {:<16} {:<16} {:<6} {:<6}",
                    "manifest", "erasure", "compression", "e_fb", "c_fb"
                );
                for entry in entries {
                    let manifest = entry
                        .get("manifest")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let erasure = entry.get("erasure").and_then(|v| v.as_str()).unwrap_or("-");
                    let compression = entry
                        .get("compression")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let compression_level = entry.get("compression_level").and_then(|v| v.as_i64());
                    let erasure_fb = entry
                        .get("erasure_fallback")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let compression_fb = entry
                        .get("compression_fallback")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let mut erasure_display = erasure.to_string();
                    if erasure_fb {
                        erasure_display.push('*');
                    }
                    let mut compression_display = if let Some(level) = compression_level {
                        format!("{compression}({level})")
                    } else {
                        compression.to_string()
                    };
                    if compression_fb {
                        compression_display.push('*');
                    }
                    println!(
                        "{:<64} {:<16} {:<16} {:<6} {:<6}",
                        manifest,
                        erasure_display,
                        compression_display,
                        if erasure_fb { "yes" } else { "no" },
                        if compression_fb { "yes" } else { "no" }
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
    }
}
