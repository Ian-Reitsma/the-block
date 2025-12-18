# Tactical Implementation Guide: Orchard + The-Block Integration

**Purpose:** Step-by-step engineering guide to implement GPU acceleration  
**Audience:** Engineering team executing the integration  
**Scope:** Code structure, APIs, testing patterns, deployment procedures

---

## Phase 1: Signature Verification Kernel (Weeks 1–2)

### Orchard Side: Metal Kernel Implementation

#### Step 1.1: Create Kernel File

```bash
# File: metal-tensor/metal/kernels/BatchPointScalarMul.metal

#include <metal_stdlib>
using namespace metal;

// Ed25519 point (compressed)
struct Point {
    uchar data[32];
};

// Scalar for multiplication
struct Scalar {
    uchar data[32];
};

// Output: result of point * scalar
struct ResultPoint {
    uchar data[32];  // Compressed result
    bool valid;      // Whether computation succeeded
};

// Batch point scalar multiplication kernel
// Each thread processes one (point, scalar) pair
kernel void batch_point_scalar_mul(
    device const Point* points [[buffer(0)]],
    device const Scalar* scalars [[buffer(1)]],
    device ResultPoint* results [[buffer(2)]],
    uint id [[thread_position_in_grid]]
) {
    // Note: Actual Ed25519 implementation is complex; this is a placeholder.
    // In practice, you'd:
    // 1. Decompress the point (if compressed)
    // 2. Load scalar bits
    // 3. Perform Montgomery ladder or similar
    // 4. Compress result
    // 5. Write to results[id]
    
    // Simplified pseudocode:
    Point p = points[id];
    Scalar s = scalars[id];
    
    // Actual Ed25519 math here...
    // (This is highly non-trivial; consider using an existing library
    // or a well-tested implementation)
    
    results[id].valid = true;  // Optimistically
}
```

#### Step 1.2: Integrate into Orchard Runtime

Create a C++ wrapper in `metal/runtime/`:

```cpp
// File: metal-tensor/metal/runtime/SignatureKernels.mm

#include "SignatureKernels.h"
#include <Metal/Metal.h>
#include <memory>
#include <vector>

class SignatureKernel {
private:
    MTLDevice* device;
    MTLLibrary* library;
    MTLComputePipelineState* pipelineState;
    MTLCommandQueue* commandQueue;

public:
    SignatureKernel() {
        device = MTLCreateSystemDefaultDevice();
        if (!device) throw std::runtime_error("Metal device not available");
        
        // Compile the shader
        NSError* error = nullptr;
        NSString* kernelPath = [NSString stringWithUTF8String:
            std::getenv("ORCHARD_KERNEL_DIR")];
        NSString* kernelFile = [kernelPath stringByAppendingPathComponent:
            @"BatchPointScalarMul.metal"];
        NSString* source = [NSString stringWithContentsOfFile:kernelFile
            encoding:NSUTF8StringEncoding error:&error];
        if (error) throw std::runtime_error("Failed to load kernel source");
        
        library = [device newLibraryWithSource:source options:nullptr error:&error];
        if (error) throw std::runtime_error("Failed to compile kernel");
        
        MTLFunction* kernelFunc = [library newFunctionWithName:@"batch_point_scalar_mul"];
        pipelineState = [device newComputePipelineStateWithFunction:kernelFunc error:&error];
        if (error) throw std::runtime_error("Failed to create pipeline");
        
        commandQueue = [device newCommandQueue];
    }

    std::vector<bool> batch_verify_signatures(
        const std::vector<std::vector<uint8_t>>& messages,
        const std::vector<std::vector<uint8_t>>& public_keys,
        const std::vector<std::vector<uint8_t>>& signatures
    ) {
        size_t batch_size = messages.size();
        
        // Allocate GPU buffers
        MTLBuffer* msgBuffer = [device newBufferWithBytes:messages.data()
            length:batch_size * 32 options:MTLResourceStorageModeShared];
        MTLBuffer* pkBuffer = [device newBufferWithBytes:public_keys.data()
            length:batch_size * 32 options:MTLResourceStorageModeShared];
        MTLBuffer* resultBuffer = [device newBufferWithLength:batch_size * sizeof(bool)
            options:MTLResourceStorageModeShared];
        
        // Dispatch kernel
        MTLCommandBuffer* cmdBuffer = [commandQueue commandBuffer];
        MTLComputeCommandEncoder* encoder = [cmdBuffer computeCommandEncoder];
        [encoder setComputePipelineState:pipelineState];
        [encoder setBuffer:msgBuffer offset:0 atIndex:0];
        [encoder setBuffer:pkBuffer offset:0 atIndex:1];
        [encoder setBuffer:resultBuffer offset:0 atIndex:2];
        
        MTLSize gridSize = MTLSizeMake(batch_size, 1, 1);
        MTLSize threadGroupSize = MTLSizeMake(256, 1, 1);
        [encoder dispatchThreads:gridSize threadsPerThreadgroup:threadGroupSize];
        [encoder endEncoding];
        
        [cmdBuffer commit];
        [cmdBuffer waitUntilCompleted];
        
        // Read results
        bool* results = (bool*)[resultBuffer contents];
        return std::vector<bool>(results, results + batch_size);
    }
};
```

#### Step 1.3: Add Header & Export

```cpp
// File: metal-tensor/metal/runtime/SignatureKernels.h

#ifndef ORCHARD_SIGNATURE_KERNELS_H
#define ORCHARD_SIGNATURE_KERNELS_H

#include <vector>
#include <cstdint>

extern "C" {
    typedef struct {
        std::vector<bool>* results;  // Output results
        int error_code;              // 0 = success
    } SignatureVerifyResult;

    SignatureVerifyResult orchard_batch_verify_signatures(
        const uint8_t* const* messages,
        const size_t* message_lengths,
        const uint8_t* const* public_keys,
        const uint8_t* const* signatures,
        size_t batch_size
    );
}

#endif
```

#### Step 1.4: Update Orchard CMakeLists

In `metal-tensor/CMakeLists.txt`, add:

```cmake
add_library(orchard_signatures STATIC
    metal/runtime/SignatureKernels.mm
)

target_include_directories(orchard_signatures PUBLIC 
    ${CMAKE_CURRENT_SOURCE_DIR}/metal)

target_link_libraries(orchard_signatures PUBLIC 
    Metal::Metal objc
)

# Add to main library
target_link_libraries(orchard_metal PUBLIC orchard_signatures)
```

#### Step 1.5: Benchmarking Harness

```cpp
// File: benchmarks/bench_signatures.cpp

#include <benchmark/benchmark.h>
#include "metal/runtime/SignatureKernels.h"
#include <vector>
#include <random>

static void BenchSignatureVerifyGPU(benchmark::State& state) {
    size_t batch_size = state.range(0);
    
    // Generate test data
    std::vector<std::vector<uint8_t>> messages(batch_size, std::vector<uint8_t>(32));
    std::vector<std::vector<uint8_t>> public_keys(batch_size, std::vector<uint8_t>(32));
    std::vector<std::vector<uint8_t>> signatures(batch_size, std::vector<uint8_t>(64));
    
    std::mt19937 gen(42);
    std::uniform_int_distribution<> dis(0, 255);
    for (auto& msg : messages) {
        for (auto& byte : msg) byte = dis(gen);
    }
    // (Similar for pks and sigs)
    
    for (auto _ : state) {
        auto result = orchard_batch_verify_signatures(
            (const uint8_t* const*)messages.data(),
            nullptr,  // lengths all 32
            (const uint8_t* const*)public_keys.data(),
            (const uint8_t* const*)signatures.data(),
            batch_size
        );
        benchmark::DoNotOptimize(result);
    }
}

BENCHMARK(BenchSignatureVerifyGPU)
    ->Arg(1)
    ->Arg(64)
    ->Arg(256)
    ->Arg(1024);
```

---

### The-Block Side: Rust FFI & Integration

#### Step 1.6: Create FFI Module

```rust
// File: crates/runtime/src/accelerator/ffi.rs

use std::ffi::c_int;

#[link(name = "orchard_metal", kind = "static")]
extern "C" {
    pub fn orchard_batch_verify_signatures(
        messages: *const *const u8,
        message_lengths: *const usize,
        public_keys: *const *const u8,
        signatures: *const *const u8,
        batch_size: usize,
        out_results: *mut bool,
    ) -> c_int;

    pub fn orchard_available() -> bool;
}

#[derive(Debug)]
pub enum FfiError {
    BatchVerifyFailed(i32),
    BufferAllocationFailed,
}

pub fn batch_verify_signatures_ffi(
    messages: &[&[u8]],
    public_keys: &[&[u8]],
    signatures: &[&[u8]],
) -> Result<Vec<bool>, FfiError> {
    let batch_size = messages.len();
    if batch_size == 0 {
        return Ok(vec![]);
    }

    // Convert Rust slices to FFI-compatible pointers
    let msg_ptrs: Vec<*const u8> = messages.iter().map(|m| m.as_ptr()).collect();
    let pk_ptrs: Vec<*const u8> = public_keys.iter().map(|pk| pk.as_ptr()).collect();
    let sig_ptrs: Vec<*const u8> = signatures.iter().map(|s| s.as_ptr()).collect();

    let mut results = vec![false; batch_size];

    unsafe {
        let error_code = orchard_batch_verify_signatures(
            msg_ptrs.as_ptr(),
            std::ptr::null(),  // All messages are 32 bytes
            pk_ptrs.as_ptr(),
            sig_ptrs.as_ptr(),
            batch_size,
            results.as_mut_ptr(),
        );

        if error_code != 0 {
            return Err(FfiError::BatchVerifyFailed(error_code));
        }
    }

    Ok(results)
}
```

#### Step 1.7: Accelerator Trait

```rust
// File: crates/runtime/src/accelerator/mod.rs

mod ffi;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcceleratorError {
    #[error("GPU acceleration failed: {0}")]
    GpuFailure(String),
    #[error("GPU not available")]
    NotAvailable,
}

pub trait GpuAccelerator: Send + Sync {
    fn batch_verify_signatures(
        &self,
        messages: &[&[u8]],
        public_keys: &[&[u8]],
        signatures: &[&[u8]],
    ) -> Result<Vec<bool>, AcceleratorError>;

    fn is_available(&self) -> bool;
}

/// CPU-only implementation (always works)
pub struct CpuAccelerator;

impl GpuAccelerator for CpuAccelerator {
    fn batch_verify_signatures(
        &self,
        messages: &[&[u8]],
        public_keys: &[&[u8]],
        signatures: &[&[u8]],
    ) -> Result<Vec<bool>, AcceleratorError> {
        // Use ed25519-zebra or similar
        Ok(messages
            .iter()
            .zip(public_keys.iter())
            .zip(signatures.iter())
            .map(|((msg, pk), sig)| {
                // CPU verification logic
                verify_signature_cpu(msg, pk, sig)
            })
            .collect())
    }

    fn is_available(&self) -> bool {
        true  // CPU always available
    }
}

/// GPU-accelerated implementation
pub struct GpuAccelerator;

impl GpuAccelerator for GpuAccelerator {
    fn batch_verify_signatures(
        &self,
        messages: &[&[u8]],
        public_keys: &[&[u8]],
        signatures: &[&[u8]],
    ) -> Result<Vec<bool>, AcceleratorError> {
        ffi::batch_verify_signatures_ffi(messages, public_keys, signatures)
            .map_err(|e| AcceleratorError::GpuFailure(format!("{:?}", e)))
    }

    fn is_available(&self) -> bool {
        unsafe { ffi::orchard_available() }
    }
}

/// Factory function
pub fn create_accelerator(prefer_gpu: bool) -> Box<dyn GpuAccelerator> {
    if prefer_gpu && GpuAccelerator.is_available() {
        Box::new(GpuAccelerator)
    } else {
        Box::new(CpuAccelerator)
    }
}
```

#### Step 1.8: Integrate into Signature Verification

```rust
// File: crypto/src/verify.rs (modified)

use crate::runtime::accelerator::GpuAccelerator;

pub fn verify_block_signatures(
    accelerator: &dyn GpuAccelerator,
    block: &Block,
) -> bool {
    // Extract signature data from block
    let messages: Vec<&[u8]> = block.transactions.iter()
        .map(|tx| tx.message_hash.as_slice())
        .collect();
    
    let public_keys: Vec<&[u8]> = block.transactions.iter()
        .map(|tx| tx.public_key.as_slice())
        .collect();
    
    let signatures: Vec<&[u8]> = block.transactions.iter()
        .map(|tx| tx.signature.as_slice())
        .collect();

    // Try GPU first; fallback to CPU
    match accelerator.batch_verify_signatures(&messages, &public_keys, &signatures) {
        Ok(results) => results.iter().all(|&r| r),
        Err(_) => {
            tracing::warn!("GPU verification failed; falling back to CPU");
            verify_block_signatures_cpu(block)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_cpu_equivalence() {
        let block = create_test_block();
        let gpu_acc = GpuAccelerator;
        let cpu_acc = CpuAccelerator;

        let gpu_result = verify_block_signatures(&gpu_acc, &block);
        let cpu_result = verify_block_signatures(&cpu_acc, &block);

        assert_eq!(gpu_result, cpu_result, "GPU and CPU must produce identical results");
    }
}
```

#### Step 1.9: Benchmarking in The-Block

```rust
// File: crates/runtime/benches/sig_verify.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use the_block_runtime::accelerator::*;

fn bench_signatures(c: &mut Criterion) {
    let gpu_acc = GpuAccelerator;
    let cpu_acc = CpuAccelerator;

    c.bench_function("gpu_verify_256_sigs", |b| {
        let (msgs, pks, sigs) = generate_test_data(256);
        b.iter(|| {
            gpu_acc.batch_verify_signatures(
                black_box(&msgs),
                black_box(&pks),
                black_box(&sigs),
            )
        });
    });

    c.bench_function("cpu_verify_256_sigs", |b| {
        let (msgs, pks, sigs) = generate_test_data(256);
        b.iter(|| {
            cpu_acc.batch_verify_signatures(
                black_box(&msgs),
                black_box(&pks),
                black_box(&sigs),
            )
        });
    });
}

criterion_group!(benches, bench_signatures);
criterion_main!(benches);
```

---

## Phase 2: Build & Linking Configuration (Week 2)

### Step 2.1: Update The-Block Cargo.toml

```toml
# In the-block root Cargo.toml

[build-dependencies]
cc = "1.0"

[features]
default = ["metal-acceleration"]
metal-acceleration = []

[target.'cfg(target_os = "macos")'.dependencies]
# No explicit dependency on Orchard (it's linked via build script)
```

### Step 2.2: Build Script

```rust
// File: build.rs (in the-block root)

use std::env;
use std::path::PathBuf;

fn main() {
    let metal_enabled = cfg!(target_os = "macos") &&
        env::var("CARGO_FEATURE_METAL_ACCELERATION").is_ok();

    if metal_enabled {
        let orchard_path = PathBuf::from(env::var("ORCHARD_PATH")
            .unwrap_or_else(|_| "../Apple-Metal-Orchard".to_string()));

        // Build Orchard
        println!("cargo:warning=Building Apple-Metal-Orchard...");
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!(
                "cd {} && mkdir -p build && cd build && \
                 cmake .. -G Ninja && cmake --build . --target orchard_metal",
                orchard_path.display()
            ))
            .output()
            .expect("Failed to build Orchard");

        if !output.status.success() {
            eprintln!("Orchard build failed: {}", String::from_utf8_lossy(&output.stderr));
            panic!("Orchard build failed");
        }

        // Link Orchard
        let lib_path = orchard_path.join("build/metal-tensor");
        println!("cargo:rustc-link-search=native={}", lib_path.display());
        println!("cargo:rustc-link-lib=static=orchard_metal");
        println!("cargo:rustc-link-lib=static=orchard_core");

        // Link system frameworks
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=Accelerate");

        // Re-run if Orchard changes
        println!("cargo:rerun-if-changed={}", orchard_path.display());
    }
}
```

### Step 2.3: Conditional Compilation

```rust
// File: crates/runtime/src/lib.rs

#[cfg(all(target_os = "macos", feature = "metal-acceleration"))]
pub mod accelerator;

#[cfg(not(all(target_os = "macos", feature = "metal-acceleration")))]
pub mod accelerator {
    // CPU-only fallback
    pub struct CpuAccelerator;
    pub fn create_accelerator(_prefer_gpu: bool) -> Box<dyn GpuAccelerator> {
        Box::new(CpuAccelerator)
    }
    // ...
}
```

---

## Phase 3: Testing Strategy (Weeks 3–4)

### Step 3.1: Unit Tests (Correctness)

```rust
// File: crates/runtime/src/accelerator/tests.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_batch() {
        let gpu = GpuAccelerator;
        let result = gpu.batch_verify_signatures(&[], &[], &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![]);
    }

    #[test]
    fn test_single_signature() {
        let gpu = GpuAccelerator;
        let cpu = CpuAccelerator;

        let (msgs, pks, sigs) = generate_test_data(1);

        let gpu_result = gpu.batch_verify_signatures(&msgs, &pks, &sigs);
        let cpu_result = cpu.batch_verify_signatures(&msgs, &pks, &sigs);

        assert_eq!(gpu_result, cpu_result);
    }

    #[test]
    fn test_batch_256() {
        let gpu = GpuAccelerator;
        let cpu = CpuAccelerator;

        let (msgs, pks, sigs) = generate_test_data(256);

        let gpu_result = gpu.batch_verify_signatures(&msgs, &pks, &sigs);
        let cpu_result = cpu.batch_verify_signatures(&msgs, &pks, &sigs);

        assert_eq!(gpu_result, cpu_result, "GPU and CPU must match");
    }

    #[test]
    fn test_fallback_on_gpu_failure() {
        // Simulate GPU failure
        let result = match gpu.batch_verify_signatures(&[], &[], &[]) {
            Ok(_) => true,
            Err(_) => {
                // Fall back to CPU
                cpu.batch_verify_signatures(&[], &[], &[]).is_ok()
            }
        };
        assert!(result);
    }
}
```

### Step 3.2: Consensus Tests

```rust
// File: tests/gpu_consensus.rs

#[tokio::test]
async fn test_block_validation_gpu_cpu_equivalence() {
    let block = create_valid_block();

    let gpu_validator = ValidatorNode::new(GpuAccelerator);
    let cpu_validator = ValidatorNode::new(CpuAccelerator);

    let gpu_valid = gpu_validator.validate_block(&block).await;
    let cpu_valid = cpu_validator.validate_block(&block).await;

    assert_eq!(gpu_valid, cpu_valid, "Consensus split detected!");
}

#[tokio::test]
async fn test_block_sequence_determinism() {
    let blocks = create_valid_chain(100);
    let gpu_validator = ValidatorNode::new(GpuAccelerator);
    let cpu_validator = ValidatorNode::new(CpuAccelerator);

    for block in blocks {
        let gpu_valid = gpu_validator.validate_block(&block).await;
        let cpu_valid = cpu_validator.validate_block(&block).await;
        assert_eq!(gpu_valid, cpu_valid, "Consensus split on block {:?}", block.hash());
    }
}
```

### Step 3.3: Stress Tests

```rust
// File: tests/gpu_stress.rs

#[tokio::test]
async fn test_high_throughput_batches() {
    let gpu = GpuAccelerator;
    let cpu = CpuAccelerator;

    for batch_size in [64, 256, 1024, 4096] {
        let (msgs, pks, sigs) = generate_test_data(batch_size);

        let gpu_result = gpu.batch_verify_signatures(&msgs, &pks, &sigs);
        let cpu_result = cpu.batch_verify_signatures(&msgs, &pks, &sigs);

        assert_eq!(gpu_result, cpu_result, "Mismatch at batch size {}", batch_size);
    }
}

#[tokio::test]
async fn test_repeated_calls() {
    let gpu = GpuAccelerator;
    for _ in 0..1000 {
        let (msgs, pks, sigs) = generate_test_data(256);
        let _ = gpu.batch_verify_signatures(&msgs, &pks, &sigs);
    }
    // If we get here without crashing, test passes
}

#[tokio::test]
async fn test_gpu_memory_pressure() {
    let gpu = GpuAccelerator;
    
    // Try very large batch sizes
    for batch_size in [8192, 16384, 32768] {
        let (msgs, pks, sigs) = generate_test_data(batch_size);
        let result = gpu.batch_verify_signatures(&msgs, &pks, &sigs);
        
        if result.is_err() {
            // Expected on large batches; should fallback gracefully
            println!("GPU batch {} exhausted: falling back to CPU", batch_size);
        }
    }
}
```

---

## Phase 4: Monitoring & Telemetry (Week 4)

### Step 4.1: Metrics Collection

```rust
// File: crates/runtime/src/accelerator/metrics.rs

use prometheus::{Counter, Histogram, IntGauge};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref GPU_VERIFY_TIME: Histogram = Histogram::new(
        "gpu_signature_verify_duration_ms",
        "Time to verify signatures on GPU"
    ).unwrap();

    pub static ref CPU_VERIFY_TIME: Histogram = Histogram::new(
        "cpu_signature_verify_duration_ms",
        "Time to verify signatures on CPU"
    ).unwrap();

    pub static ref GPU_VERIFY_FAILURES: Counter = Counter::new(
        "gpu_signature_verify_failures",
        "Number of GPU verification failures"
    ).unwrap();

    pub static ref GPU_BATCH_SIZE: Histogram = Histogram::new(
        "gpu_batch_size",
        "Batch size for GPU verification"
    ).unwrap();
}

pub fn record_gpu_verification(
    batch_size: usize,
    duration_ms: f64,
    success: bool,
) {
    GPU_BATCH_SIZE.observe(batch_size as f64);
    GPU_VERIFY_TIME.observe(duration_ms);
    if !success {
        GPU_VERIFY_FAILURES.inc();
    }
}
```

### Step 4.2: Grafana Dashboard

```json
{
  "dashboard": {
    "title": "GPU Acceleration Metrics",
    "panels": [
      {
        "title": "GPU vs CPU Verification Time",
        "targets": [
          {
            "expr": "rate(gpu_signature_verify_duration_ms[5m])"
          },
          {
            "expr": "rate(cpu_signature_verify_duration_ms[5m])"
          }
        ]
      },
      {
        "title": "GPU Failure Rate",
        "targets": [
          {
            "expr": "rate(gpu_signature_verify_failures[5m])"
          }
        ]
      },
      {
        "title": "Average Batch Size",
        "targets": [
          {
            "expr": "avg(gpu_batch_size)"
          }
        ]
      }
    ]
  }
}
```

---

## Phase 5: Deployment Checklist (Week 5+)

### Pre-Release

- [ ] All unit tests pass on macOS
- [ ] All consensus tests pass (GPU == CPU)
- [ ] Benchmark suite runs successfully
- [ ] CPU-only builds still work (Linux CI)
- [ ] Documentation updated
- [ ] Performance report generated

### Testnet Deployment

- [ ] Deploy shadow mode (GPU computes, results not used)
- [ ] Monitor for 24 hours (no crashes, memory stable)
- [ ] Collect telemetry
- [ ] Publish preliminary results

### Mainnet Deployment

- [ ] Staged rollout (opt-in first)
- [ ] Community communication
- [ ] 24/7 monitoring
- [ ] Prepared rollback procedure

---

## Testing Command Reference

```bash
# Build Orchard
cd ../Apple-Metal-Orchard
cmake -S . -B build -G Ninja
cmake --build build

# Build the-block with GPU support
cd ../../the-block
OARCHARD_PATH=../Apple-Metal-Orchard cargo build --features metal-acceleration

# Run tests
cargo test --features metal-acceleration -- --nocapture

# Run benchmarks
cargo bench --features metal-acceleration

# CPU-only build (should still work)
cargo build

# Enable GPU at runtime
OARCHARD_METAL=1 cargo run
```

---

**Status:** Ready for implementation  
**Last Updated:** December 2025
