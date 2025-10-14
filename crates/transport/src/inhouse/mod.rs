mod adapter;
mod certificate;
mod messages;
mod store;

pub use adapter::{
    certificate_store, verify_remote_certificate, Adapter, ConnectOutcome, Connection,
    ConnectionStatsSnapshot, Endpoint, InhouseEventCallbacks,
};
pub use certificate::{fingerprint, fingerprint_history, Certificate};
pub use store::{Advertisement, InhouseCertificateStore};

pub const PROVIDER_ID: &str = adapter::PROVIDER_ID;
pub const CAPABILITIES: &[crate::ProviderCapability] = adapter::CAPABILITIES;
