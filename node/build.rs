use std::env;
use std::fs;
use std::path::PathBuf;

use dependency_guard::{panic_on_failure, rerun_if_env_changed};

fn main() {
    rerun_if_env_changed();
    panic_on_failure(dependency_guard::enforce_current_crate());

    emit_build_hash();
    write_genesis_stub();
}

fn emit_build_hash() {
    println!("cargo:rustc-env=BUILD_BIN_HASH=FIRST_PARTY_FREEZE");
}

fn write_genesis_stub() {
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let mut path = PathBuf::from(out_dir);
        path.push("genesis_hash.txt");
        const PLACEHOLDER: &str =
            "141d37794bb13248195df2da120fd5dec5db53ef1e13963f68dafdce0c4abb95";
        if let Err(err) = fs::write(&path, PLACEHOLDER) {
            panic!("failed to write genesis hash stub: {err}");
        }
    }
}
