use std::process::Command;

fn main() {
    if let Ok(out) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                println!("cargo:rustc-env=BUILD_BIN_HASH={}", s.trim());
            }
        }
    }
}
