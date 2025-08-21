use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use the_block::governance::{Governance, House};

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
    let mut gov = Governance::new(1, 1, 0);
    match cli.cmd {
        Command::Submit { file } => {
            let text = fs::read_to_string(file).expect("read");
            let v: serde_json::Value = serde_json::from_str(&text).expect("json");
            let start = v["start"].as_u64().unwrap_or(0);
            let end = v["end"].as_u64().unwrap_or(0);
            let id = gov.submit(start, end);
            println!("submitted {id}");
        }
        Command::Vote { id, house } => {
            gov.vote(id, house.into(), true).expect("vote");
            println!("voted {id}");
        }
    }
}
