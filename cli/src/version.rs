use cli_core::{
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::hashing::blake3::hash;

pub enum VersionCmd {
    /// Display build provenance info
    Provenance,
}

impl VersionCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("version"), "version", "Version and build info")
            .subcommand(
                CommandBuilder::new(
                    CommandId("version.provenance"),
                    "provenance",
                    "Display build provenance info",
                )
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, _) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'version'".to_string())?;

        match name {
            "provenance" => Ok(VersionCmd::Provenance),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
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
