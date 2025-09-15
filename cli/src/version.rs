use blake3::hash;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum VersionCmd {
    /// Display build provenance info
    Provenance,
}

pub fn handle(cmd: VersionCmd) {
    match cmd {
        VersionCmd::Provenance => {
            let expected = env!("BUILD_BIN_HASH");
            let exe = std::env::current_exe().unwrap();
            let bytes = std::fs::read(&exe).unwrap_or_default();
            let actual = hash(&bytes).to_hex().to_string();
            println!("expected={expected}\nactual={actual}");
        }
    }
}
