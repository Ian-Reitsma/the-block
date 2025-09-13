#![forbid(unsafe_code)]

use clap::Subcommand;
use std::fs;
use std::path::PathBuf;
use the_block::compute_market::snark;

#[derive(Subcommand)]
pub enum SnarkCmd {
    /// Compile a WASM task into a SNARK circuit representation
    Compile {
        wasm: PathBuf,
        out: PathBuf,
    },
}

pub fn handle(cmd: SnarkCmd) {
    match cmd {
        SnarkCmd::Compile { wasm, out } => {
            let wasm_bytes = fs::read(wasm).expect("read wasm");
            let circuit = snark::compile_wasm(&wasm_bytes);
            fs::write(out, circuit).expect("write circuit");
        }
    }
}
