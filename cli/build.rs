use std::process::Command;

use dependency_guard::{panic_on_failure, rerun_if_env_changed};

fn main() {
    rerun_if_env_changed();
    panic_on_failure(dependency_guard::enforce_current_crate());

    if let Ok(out) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                println!("cargo:rustc-env=BUILD_BIN_HASH={}", s.trim());
            }
        }
    }
}
