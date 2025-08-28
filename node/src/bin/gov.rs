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
    /// Show proposal status
    Status { id: u64 },
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
        }
    }
}
