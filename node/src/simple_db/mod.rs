#![forbid(unsafe_code)]

#[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
mod rocks;
#[cfg(all(not(feature = "lightweight-integration"), feature = "storage-rocksdb"))]
pub use rocks::{DbDelta, SimpleDb};

#[cfg(any(feature = "lightweight-integration", not(feature = "storage-rocksdb")))]
mod memory;
#[cfg(any(feature = "lightweight-integration", not(feature = "storage-rocksdb")))]
pub use memory::{DbDelta, SimpleDb};

#[cfg(all(
    test,
    feature = "storage-rocksdb",
    not(feature = "lightweight-integration")
))]
mod memory_tests {
    include!("memory.rs");
}
