use clap::{Parser, Subcommand};
use indexer::{BlockRecord, Indexer};
use std::fs::File;
use std::sync::Arc;
use axum::{routing::get, Router, Json};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "indexer")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index blocks from a JSON file into a SQLite database.
    Index { file: String, db: String },
    /// Serve a simple HTTP explorer over the indexed database.
    Serve { db: String },
    /// Print basic stats from the database.
    Profile { db: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Index { file, db } => {
            let idx = Indexer::open(&db)?;
            let records: Vec<BlockRecord> = serde_json::from_reader(File::open(file)?)?;
            for r in records {
                idx.index_block(&r)?;
            }
        }
        Commands::Serve { db } => {
            let idx = Arc::new(Indexer::open(&db)?);
            let state = idx.clone();
            let app = Router::new().route(
                "/blocks",
                get(move || {
                    let idx = state.clone();
                    async move { Json(idx.all_blocks().unwrap_or_default()) }
                }),
            );
            let listener = TcpListener::bind("0.0.0.0:3000").await?;
            axum::serve(listener, app.into_make_service()).await?;
        }
        Commands::Profile { db } => {
            let idx = Indexer::open(&db)?;
            println!("indexed blocks: {}", idx.all_blocks()?.len());
        }
    }
    Ok(())
}
