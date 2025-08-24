use blake3::Hasher;
use std::{env, fs, path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/constants.rs");
    println!("cargo:rerun-if-env-changed=PYTHONHOME");
    let ld = Command::new("python3-config")
        .arg("--ldflags")
        .output()
        .expect("python3-config missing");
    if !ld.status.success() {
        eprintln!("::error::python3-config --ldflags failed");
        std::process::exit(1);
    }
    let flags = String::from_utf8_lossy(&ld.stdout);
    let lib_path = flags.split_whitespace().find_map(|s| s.strip_prefix("-L"));
    if let Some(p) = lib_path {
        println!("cargo:rustc-link-search=native={}", p);
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", p);
    } else {
        eprintln!("::error::unable to locate Python shared library");
        std::process::exit(1);
    }
    if cfg!(target_os = "macos") {
        if let Ok(py_home) = env::var("PYTHONHOME") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", py_home);
        }
    }
    if !include_str!("src/constants.rs").is_ascii() {
        println!(
            "::error file=src/constants.rs,line=1,col=1::Non-ASCII detected in consensus file"
        );
        std::process::exit(1);
    }

    // Compute genesis hash at build time so tests can assert it at compile time.
    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    let mut h = Hasher::new();
    h.update(&0u64.to_le_bytes()); // index
    h.update(ZERO_HASH.as_bytes()); // prev
    h.update(&0u64.to_le_bytes()); // nonce
    h.update(&8u64.to_le_bytes()); // difficulty
    h.update(&0u64.to_le_bytes()); // coin_c
    h.update(&0u64.to_le_bytes()); // coin_i
    h.update(ZERO_HASH.as_bytes()); // fee checksum
                                    // no tx ids in genesis
    let digest = h.finalize().to_hex().to_string();

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR missing");
    let dest = Path::new(&out_dir).join("genesis_hash.txt");
    fs::write(dest, digest).expect("write genesis hash");
}
