#![allow(clippy::expect_used)]

use clap::{Parser, Subcommand};
use the_block::{Blockchain, SnapshotManager};

#[derive(Parser)]
#[command(author, version, about = "Manage chain snapshots")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a snapshot from the given data directory
    Create { data_dir: String, #[arg(long)] db_path: Option<String> },
    /// Apply the latest snapshot and print its root
    Apply { data_dir: String, #[arg(long)] db_path: Option<String> },
    /// List available snapshot heights
    List { data_dir: String },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Create { data_dir, db_path } => {
            let dbp = db_path.unwrap_or_else(|| data_dir.clone());
            let bc = Blockchain::open_with_db(&data_dir, &dbp).expect("open chain");
            let mgr = SnapshotManager::new(data_dir, bc.snapshot.interval);
            let _ = mgr
                .write_snapshot(bc.block_height, bc.accounts())
                .expect("snapshot");
        }
        Command::Apply { data_dir, db_path: _ } => {
            let mgr = SnapshotManager::new(data_dir, 0);
            if let Ok(Some((h, _, root))) = mgr.load_latest() {
                println!("{h}:{root}");
            }
        }
        Command::List { data_dir } => {
            let mgr = SnapshotManager::new(data_dir, 0);
            if let Ok(list) = mgr.list() {
                for h in list {
                    println!("{h}");
                }
            }
        }
    }
}
