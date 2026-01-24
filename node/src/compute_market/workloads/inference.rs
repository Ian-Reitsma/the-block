use super::BlockTorchWorkloadMetadata;
use concurrency::Lazy;
use crypto_suite::hashing::blake3::Hasher;
use crypto_suite::mac::sha256_digest;
use foundation_serialization::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// BlockTorch inference payload description (artifact + input + optional metadata).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct BlockTorchInference {
    pub artifact: Vec<u8>,
    pub input: Vec<u8>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub benchmark_commit: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "foundation_serialization::skip::option_is_none")]
    pub tensor_profile_epoch: Option<String>,
}

impl BlockTorchInference {
    pub fn new(artifact: Vec<u8>, input: Vec<u8>) -> Self {
        Self {
            artifact,
            input,
            benchmark_commit: None,
            tensor_profile_epoch: None,
        }
    }

    pub fn with_benchmark_commit(mut self, commit: impl Into<String>) -> Self {
        self.benchmark_commit = Some(commit.into());
        self
    }

    pub fn with_tensor_profile_epoch(mut self, epoch: impl Into<String>) -> Self {
        self.tensor_profile_epoch = Some(epoch.into());
        self
    }

    pub fn metadata(&self) -> BlockTorchWorkloadMetadata {
        BlockTorchWorkloadMetadata {
            kernel_digest: kernel_bundle_digest(),
            benchmark_commit: self.benchmark_commit.clone(),
            tensor_profile_epoch: self.tensor_profile_epoch.clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct InferenceFeatures {
    sum: f32,
    mean: f32,
    used_elements: u64,
}

/// Execute the BlockTorch inference workload via the deterministic CPU fallback.
pub fn run(payload: &BlockTorchInference) -> [u8; 32] {
    cpu_fallback_hash(payload)
}

/// Deterministic CPU fallback that mirrors the expected runtime output.
pub fn cpu_fallback_hash(payload: &BlockTorchInference) -> [u8; 32] {
    finalize_digest(payload, &cpu_features(payload))
}

fn cpu_features(payload: &BlockTorchInference) -> InferenceFeatures {
    let used = std::cmp::min(payload.artifact.len(), payload.input.len());
    let mut sum = 0.0f32;
    if used > 0 {
        for i in 0..used {
            let a = payload.artifact[i] as f32 / 255.0;
            let b = payload.input[i] as f32 / 255.0;
            sum += a * b;
        }
    }
    let mean = if used == 0 { 0.0 } else { sum / used as f32 };
    InferenceFeatures {
        sum,
        mean,
        used_elements: used as u64,
    }
}

fn finalize_digest(payload: &BlockTorchInference, features: &InferenceFeatures) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&features.sum.to_le_bytes());
    h.update(&features.mean.to_le_bytes());
    h.update(&features.used_elements.to_le_bytes());
    h.update(&(payload.artifact.len() as u64).to_le_bytes());
    h.update(&(payload.input.len() as u64).to_le_bytes());
    h.update(&payload.artifact);
    h.update(&payload.input);
    *h.finalize().as_bytes()
}

fn kernel_bundle_digest() -> [u8; 32] {
    *KERNEL_DIGEST
}

fn compute_kernel_bundle_digest() -> [u8; 32] {
    let blocktorch_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blocktorch");

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
