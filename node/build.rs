use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FIRST_PARTY_ONLY");
    if !enforce_guard() {
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| String::from("."));
    let cargo = env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));

    let output = Command::new(cargo)
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(format!("{manifest_dir}/Cargo.toml"))
        .output()
        .expect("failed to execute cargo metadata");

    if !output.status.success() {
        panic!(
            "dependency guard failed: cargo metadata exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let offenders = detect_third_party(&stdout);
    if offenders.is_empty() {
        emit_build_hash();
        write_genesis_stub();
        return;
    }

    let mut offenders_vec: Vec<_> = offenders.into_iter().collect();
    offenders_vec.sort();
    let mut rendered = offenders_vec
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if offenders_vec.len() > 10 {
        rendered.push_str(" ...");
    }

    panic!("third-party crates detected while FIRST_PARTY_ONLY=1: {rendered}");
}

fn enforce_guard() -> bool {
    match env::var("FIRST_PARTY_ONLY") {
        Ok(value) => value != "0",
        Err(_) => true,
    }
}

fn detect_third_party(metadata: &str) -> BTreeSet<String> {
    let mut offenders = BTreeSet::new();
    let mut search_start = 0usize;
    const SOURCE_MARKER: &str = "\"source\":";
    const NAME_MARKER: &str = "\"name\":";

    while let Some(relative_idx) = metadata[search_start..].find(SOURCE_MARKER) {
        let source_idx = search_start + relative_idx + SOURCE_MARKER.len();
        let tail = &metadata[source_idx..];
        if !(tail.starts_with("\"registry+") || tail.starts_with("\"git+")) {
            search_start = source_idx;
            continue;
        }

        if let Some(prefix) = metadata[..source_idx - SOURCE_MARKER.len()].rfind(NAME_MARKER) {
            let mut name_start = prefix + NAME_MARKER.len();
            if metadata[name_start..].starts_with('"') {
                name_start += 1;
            }
            if let Some(rest) = metadata[name_start..].find('"') {
                let name = &metadata[name_start..name_start + rest];
                if !name.trim().is_empty() {
                    offenders.insert(name.trim().to_string());
                }
            }
        }

        search_start = source_idx + 1;
    }

    offenders
}

fn emit_build_hash() {
    println!("cargo:rustc-env=BUILD_BIN_HASH=FIRST_PARTY_FREEZE");
}

fn write_genesis_stub() {
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let mut path = PathBuf::from(out_dir);
        path.push("genesis_hash.txt");
        const PLACEHOLDER: &str =
            "80e68b5d4436e3a9925919c9f91e213f1e336b439a99a57070553f3b0520d1aa\n";
        if let Err(err) = fs::write(&path, PLACEHOLDER) {
            panic!("failed to write genesis hash stub: {err}");
        }
    }
}
