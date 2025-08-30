#![allow(clippy::expect_used)]
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use the_block::{governance::House, Governance};

#[derive(Parser)]
#[command(author, version, about = "Governance helpers")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Submit a proposal JSON file
    Submit { file: String },
    /// Vote for a proposal
    Vote { id: u64, house: HouseArg },
    /// Execute a proposal after quorum and timelock
    Exec { id: u64 },
    /// Show proposal status and rollback metrics
    Status { id: u64 },
    /// Roll back the last activation
    RollbackLast,
    /// Convenience helper to craft fair-share proposals
    SetFairshare {
        #[arg(help = "global max fair share in ppm (parts per million)")]
        global_max_ppm: u64,
        #[arg(help = "burst refill rate per second in ppm")]
        burst_refill_ppm: u64,
    },
    /// Convenience helper for credit decay proposals
    SetCreditDecay {
        #[arg(help = "credit decay lambda per hour in ppm")]
        lambda_ppm: u64,
    },
}

#[derive(Clone, ValueEnum)]
enum HouseArg {
    Ops,
    Builders,
}

impl From<HouseArg> for House {
    fn from(h: HouseArg) -> Self {
        match h {
            HouseArg::Ops => House::Operators,
            HouseArg::Builders => House::Builders,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let db_path = "examples/governance/proposals.db";
    if let Some(parent) = Path::new(db_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut gov = Governance::load(db_path, 1, 1, 0);
    match cli.cmd {
        Command::Submit { file } => {
            let text = fs::read_to_string(file).expect("read");
            let v: serde_json::Value = serde_json::from_str(&text).expect("json");
            let start = v["start"].as_u64().unwrap_or(0);
            let end = v["end"].as_u64().unwrap_or(0);
            let id = gov.submit(start, end);
            println!("submitted {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Vote { id, house } => {
            gov.vote(id, house.into(), true).expect("vote");
            println!("voted {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Exec { id } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            gov.execute(id, now).expect("exec");
            println!("executed {id}");
            gov.persist(db_path).expect("persist");
        }
        Command::Status { id } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (p, remaining) = gov.status(id, now).expect("status");
            println!(
                "id={} ops_for={} builders_for={} executed={} timelock_remaining={}s",
                p.id, p.ops_for, p.builders_for, p.executed, remaining
            );
            #[cfg(feature = "telemetry")]
            {
                use the_block::telemetry::{PARAM_CHANGE_ACTIVE, PARAM_CHANGE_PENDING};
                let params = [
                    "snapshot_interval_secs",
                    "consumer_fee_comfort_p90_microunits",
                    "industrial_admission_min_capacity",
                ];
                for name in params {
                    let active = PARAM_CHANGE_ACTIVE.with_label_values(&[name]).get();
                    let pending = PARAM_CHANGE_PENDING.with_label_values(&[name]).get();
                    println!("param={name} active={active} pending={pending}");
                }
            }
        }
        Command::RollbackLast => {
            // placeholder for future expanded CLI
            println!("rollback not supported in simple governance");
        }
        Command::SetFairshare {
            global_max_ppm,
            burst_refill_ppm,
        } => {
            println!(
                "submit proposal with key fairshare_global_max_ppm={} or burst_refill_rate_per_s_ppm={}",
                global_max_ppm, burst_refill_ppm
            );
        }
        Command::SetCreditDecay { lambda_ppm } => {
            println!(
                "submit proposal with key credits_decay_lambda_per_hour_ppm={}",
                lambda_ppm
            );
        }
    }
}
