use crate::transaction::FeeLane;
use serde::{Deserialize, Serialize};

/// Manifest transaction mapping a domain to blob identifiers.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SiteManifestTx {
    /// Account that controls the domain.
    pub owner: String,
    /// Fully qualified domain name being published.
    pub domain: String,
    /// BLAKE3 or KZG root of the manifest JSON blob.
    pub root_blob: [u8; 32],
    /// Optional dynamic entry point referencing a [`FuncTx`].
    pub dyn_entry: Option<[u8; 32]>,
    /// Whether the site is public or requires client auth.
    pub public: bool,
}

impl SiteManifestTx {
    pub fn lane(&self) -> FeeLane {
        FeeLane::Consumer
    }
}

/// WebAssembly function transaction used for dynamic endpoints.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct FuncTx {
    /// Owner account identifier.
    pub owner: String,
    /// Unique function identifier.
    pub func_id: [u8; 32],
    /// Commitment to the WASM bytecode blob.
    pub wasm_root: [u8; 32],
    /// Maximum fuel units the function may consume per call.
    pub gas_limit: u32,
    /// HTTP paths served by this function ("/api/*" etc.).
    pub http_paths: Vec<String>,
}

impl FuncTx {
    pub fn lane(&self) -> FeeLane {
        FeeLane::Consumer
    }
}
