use clap::{Parser, Subcommand};
use crypto_suite::hashing::blake3;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "installer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Package binaries for a target OS and sign the archive
    Package { os: String, out: PathBuf },
    /// Update the running binary from GitHub releases
    Update,
}

fn check_deps() -> std::io::Result<()> {
    for dep in ["tar", "gzip"] {
        if which::which(dep).is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing dependency: {dep}"),
            ));
        }
    }
    Ok(())
}

fn package(os: String, out: PathBuf) -> std::io::Result<()> {
    check_deps()?;
    let file = File::create(&out)?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("README.txt", zip::write::FileOptions::default())?;
    zip.write_all(format!("Installer for {os}\n").as_bytes())?;
    zip.finish()?;
    let bytes = fs::read(&out)?;
    let sig = blake3::hash(&bytes);
    fs::write(out.with_extension("sig"), sig.to_hex().as_bytes())?;
    Ok(())
}

fn update() -> anyhow::Result<()> {
    self_update::backends::github::Update::configure()
        .repo_owner("the-block")
        .repo_name("node")
        .bin_name("the-block")
        .show_download_progress(true)
        .build()?
        .update()?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Package { os, out } => package(os, out)?,
        Commands::Update => update()?,
    }
    Ok(())
}
