#![forbid(unsafe_code)]

use crate::parse_utils::require_positional;
use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use std::fs;
use std::path::PathBuf;
use the_block::compute_market::snark;

pub enum SnarkCmd {
    /// Compile a WASM task into a SNARK circuit representation
    Compile { wasm: PathBuf, out: PathBuf },
}

impl SnarkCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("snark"), "snark", "SNARK tooling")
            .subcommand(
                CommandBuilder::new(
                    CommandId("snark.compile"),
                    "compile",
                    "Compile a WASM task into a SNARK circuit representation",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "wasm",
                    "WASM module to compile",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "out",
                    "Output circuit path",
                )))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'snark'".to_string())?;

        match name {
            "compile" => {
                let wasm = PathBuf::from(require_positional(sub_matches, "wasm")?);
                let out = PathBuf::from(require_positional(sub_matches, "out")?);
                Ok(SnarkCmd::Compile { wasm, out })
            }
            other => Err(format!("unknown subcommand '{other}' for 'snark'")),
        }
    }
}

pub fn handle(cmd: SnarkCmd) {
    match cmd {
        SnarkCmd::Compile { wasm, out } => {
            let wasm_bytes = fs::read(wasm).expect("read wasm");
            let circuit = snark::compile_wasm(&wasm_bytes).expect("compile SNARK circuit");
            fs::write(out, circuit).expect("write circuit");
        }
    }
}
