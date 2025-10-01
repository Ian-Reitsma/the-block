use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rocksdb::{checkpoint::Checkpoint, DB};
/// Create a hybrid-compressed snapshot of a RocksDB database.
pub fn create_snapshot(db_path: &Path, out_path: &Path) -> Result<()> {
    let db = DB::open_default(db_path)?;
    let checkpoint = Checkpoint::new(&db)?;
    checkpoint.create_checkpoint(".snapshot_tmp")?;
    let mut buf = Vec::new();
    File::open(".snapshot_tmp")?.read_to_end(&mut buf)?;
    let compressor = coding::compressor_for("lz77-rle", 4)?;
    let encoded = compressor.compress(&buf)?;
    fs::write(out_path, &encoded)?;
    #[cfg(feature = "telemetry")]
    metrics::increment_counter!("snapshot_created_total");
    fs::remove_file(".snapshot_tmp").ok();
    Ok(())
}

/// Restore a snapshot into a RocksDB directory.
pub fn restore_snapshot(archive_path: &Path, db_path: &Path) -> Result<()> {
    let bytes = fs::read(archive_path)?;
    let compressor = coding::compressor_for("lz77-rle", 4)?;
    let decoded = compressor.decompress(&bytes)?;
    fs::write(db_path, &decoded)?;
    Ok(())
}

#[derive(Parser)]
#[command(about = "Snapshot utilities", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Create { db: String, out: String },
    Restore { archive: String, dst: String },
}

#[cfg(not(test))]
fn main() {
    let cli = Cli::parse();
    let res = match cli.cmd {
        Commands::Create { db, out } => create_snapshot(Path::new(&db), Path::new(&out)),
        Commands::Restore { archive, dst } => {
            restore_snapshot(Path::new(&archive), Path::new(&dst))
        }
    };
    if let Err(e) = res {
        #[cfg(feature = "telemetry")]
        metrics::increment_counter!("snapshot_restore_fail_total");
        eprintln!("{e}");
        std::process::exit(1);
    }
}
