extern crate foundation_serialization as serde;

mod alert_validator;
mod chaos;
mod dashboard;
mod metrics;

pub use alert_validator::{
    validate_all_alerts, validate_bridge_alerts, validate_chain_health_alerts,
    validate_dependency_registry_alerts, validate_treasury_alerts,
    ValidationError as BridgeAlertValidationError,
};
pub use chaos::{
    sign_attestation, verify_attestation, ChaosAttestation, ChaosAttestationDecodeError,
    ChaosAttestationDraft, ChaosAttestationError, ChaosModule, ChaosReadinessSnapshot,
    ChaosSiteReadiness,
};
pub use dashboard::*;
pub use metrics::{
    ensure_monitoring_recorder, install_monitoring_recorder, monitoring_metrics,
    record_snapshot_error, record_snapshot_success, reset_monitoring_metrics, MonitoringMetrics,
};
