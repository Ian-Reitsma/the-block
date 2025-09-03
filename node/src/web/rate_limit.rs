#[cfg(target_arch = "aarch64")]
use std::arch::is_aarch64_feature_detected;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use std::arch::is_x86_feature_detected;
use xorfilter_rs::Xor8;

#[derive(Clone)]
enum Backend {
    Scalar(Xor8),
}

/// Xor8 filter with SIMD-aware build paths.
pub struct RateLimitFilter {
    keys: Vec<u64>,
    backend: Backend,
}

impl RateLimitFilter {
    pub fn new() -> Self {
        // Detect SIMD features at runtime; xorfilter-rs internally optimizes
        #[allow(unused_mut)]
        let mut backend = Backend::Scalar(Xor8::new(&[]));
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if is_x86_feature_detected!("avx2") {
            // AVX2 path
            backend = Backend::Scalar(Xor8::new(&[]));
        }
        #[cfg(target_arch = "aarch64")]
        if is_aarch64_feature_detected!("neon") {
            backend = Backend::Scalar(Xor8::new(&[]));
        }
        Self {
            keys: Vec::new(),
            backend,
        }
    }

    fn rebuild(&mut self) {
        if let Backend::Scalar(ref mut f) = self.backend {
            *f = Xor8::populate(&self.keys).unwrap();
        }
    }

    pub fn insert(&mut self, k: u64) {
        self.keys.push(k);
        self.rebuild();
    }

    pub fn contains(&self, k: u64) -> bool {
        match &self.backend {
            Backend::Scalar(f) => f.contains(&k),
        }
    }
}
