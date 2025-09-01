use clap::{Parser, Subcommand};
use the_block::SimpleDb;

#[derive(Parser)]
#[command(author, version, about = "Inspect node state and transactions")]
struct Cli {
    #[arg(long, default_value = "node-data")]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch a value by key
    Get { key: String },
    /// List keys with a prefix
    Keys { prefix: String },
}

fn main() {
    let cli = Cli::parse();
    let db = SimpleDb::open(&cli.db);
    match cli.cmd {
        Cmd::Get { key } => {
            if let Some(v) = db.get(&key) {
                println!("{}", hex::encode(v));
            } else {
                eprintln!("key not found");
            }
        }
        Cmd::Keys { prefix } => {
            for k in db.keys_with_prefix(&prefix) {
                println!("{k}");
            }
        }
    }
}
