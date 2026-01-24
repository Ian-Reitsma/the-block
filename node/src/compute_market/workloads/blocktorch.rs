use concurrency::Lazy;
use crypto_suite::mac::sha256_digest;
use std::fs;
use std::path::{Path, PathBuf};

/// Add two float slices using the BlockTorch CPU context (Metal when present).
pub fn add(left: &[f32], right: &[f32], out: &mut [f32]) -> bool {
    let len = left.len().min(right.len()).min(out.len());
    for i in 0..len {
        out[i] = left[i] + right[i];
    }
    true
}

/// Kernel bundle digest built from the blocktorch tree.
pub fn kernel_bundle_digest() -> [u8; 32] {
    *KERNEL_DIGEST
}

/// Discover the blocktorch commit hash (if the git metadata is available).
pub fn runtime_benchmark_commit() -> Option<String> {
    let root = blocktorch_root();
    let git_dir = root.join(".git");
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    if let Some(path) = head.strip_prefix("ref:") {
        let ref_path = path.trim();
        return fs::read_to_string(git_dir.join(ref_path))
            .ok()
            .map(|s| s.trim().to_string());
    }
    Some(head.to_string())
}

fn blocktorch_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blocktorch")
}

fn compute_kernel_bundle_digest() -> [u8; 32] {
    let blocktorch_root = blocktorch_root();

    let mut buffer = Vec::new();
    add_kernel_dir(
        &blocktorch_root.join("metal-tensor/metal/kernels"),
        &mut buffer,
    );
    add_file(
        &blocktorch_root.join("build/metal-tensor/CMakeCache.txt"),
        b"cmake-cache",
        &mut buffer,
    );
    add_file(
        &blocktorch_root.join("metal-tensor/CMakeLists.txt"),
        b"metal-tensor-cmake",
        &mut buffer,
    );
    add_file(
        &blocktorch_root.join("CMakeLists.txt"),
        b"orchard-root-cmake",
        &mut buffer,
    );
    add_runtime_version(
        &blocktorch_root.join("metal-tensor/runtime_version.txt"),
        &mut buffer,
    );

    if buffer.is_empty() {
        return sha256_digest(b"blocktorch-kernel-bundle-missing");
    }
    sha256_digest(&buffer)
}

fn add_kernel_dir(dir: &Path, buffer: &mut Vec<u8>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();
    files.sort();

    for path in files {
        add_file(&path, b"kernel", buffer);
    }
}

fn add_file(path: &Path, label: &[u8], buffer: &mut Vec<u8>) {
    if !path.exists() {
        return;
    }
    if let Ok(bytes) = fs::read(path) {
        buffer.extend_from_slice(label);
        buffer.extend_from_slice(path.to_string_lossy().as_bytes());
        buffer.extend_from_slice(&bytes);
    }
}

fn add_runtime_version(path: &Path, buffer: &mut Vec<u8>) {
    const RUNTIME_LABEL: &[u8] = b"runtime-version";
    if let Ok(contents) = fs::read_to_string(path) {
        let version = contents.trim();
        if version.is_empty() {
            return;
        }
        buffer.extend_from_slice(RUNTIME_LABEL);
        buffer.extend_from_slice(version.as_bytes());
    }
}

static KERNEL_DIGEST: Lazy<[u8; 32]> = Lazy::new(compute_kernel_bundle_digest);
