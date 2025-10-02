use clap::Subcommand;
use crypto_suite::hashing::blake3::hash;

#[derive(Subcommand)]
pub enum VersionCmd {
    /// Display build provenance info
    Provenance,
}

pub fn handle(cmd: VersionCmd) {
    match cmd {
        VersionCmd::Provenance => provenance(),
    }
}

pub fn provenance() {
    let expected = env!("BUILD_BIN_HASH");
    let exe = std::env::current_exe().unwrap();
    let bytes = std::fs::read(&exe).unwrap_or_default();
    let actual = hash(&bytes).to_hex().to_string();
    println!("expected={expected}\nactual={actual}");
}
