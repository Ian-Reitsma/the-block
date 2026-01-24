use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use dependency_guard::{panic_on_failure, rerun_if_env_changed};

fn main() {
    rerun_if_env_changed();
    panic_on_failure(dependency_guard::enforce_current_crate());

    build_blocktorch_bridge();
    emit_build_hash();
    write_genesis_stub();
}

fn build_blocktorch_bridge() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let source = manifest_dir.join("src").join("blocktorch_bridge.cc");
    let include_dir = manifest_dir.join("../blocktorch/metal-tensor/metal");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let obj = out_dir.join("blocktorch_bridge.o");
    let lib = out_dir.join("libblocktorch_bridge.a");
    let cxx = env::var("CXX").unwrap_or_else(|_| "c++".to_string());
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());

    println!("cargo:rerun-if-changed={}", source.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("../blocktorch/metal-tensor/metal/runtime/CpuContext.h")
            .display()
    );

    let status = Command::new(&cxx)
        .arg("-std=c++17")
        .arg("-c")
        .arg(&source)
        .arg("-I")
        .arg(&include_dir)
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("compile blocktorch_bridge.cc");
    if !status.success() {
        panic!("failed to compile blocktorch_bridge.cc with {cxx}");
    }

    let status = Command::new(&ar)
        .arg("crus")
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("archive blocktorch_bridge");
    if !status.success() {
        panic!("failed to archive blocktorch_bridge with {ar}");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=blocktorch_bridge");
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }
}

fn emit_build_hash() {
    println!("cargo:rustc-env=BUILD_BIN_HASH=FIRST_PARTY_FREEZE");
}

fn write_genesis_stub() {
    if let Ok(out_dir) = env::var("OUT_DIR") {
        let mut path = PathBuf::from(out_dir);
        path.push("genesis_hash.txt");
        const PLACEHOLDER: &str =
            "07a02d78d9b62d7fe4a32840386017fd4fba605d1c3e54b2adcf09fd91d8618d";
        if let Err(err) = fs::write(&path, PLACEHOLDER) {
            panic!("failed to write genesis hash stub: {err}");
        }
    }
}
