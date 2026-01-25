//! Canonical micro-shard root assembly and notarization pipeline.
//!
//! This module forwards the deterministic RootAssembler implementation and
//! bundle types used across block production, hashing, and manifest storage.

pub use crate::root_assembler::{
    MicroShardRootEntry, RootAssembler, RootBundle, RootBundleSummary, RootManifest, RootSizeClass,
};
