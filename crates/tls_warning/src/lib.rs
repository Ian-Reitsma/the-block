#![allow(clippy::should_implement_trait, clippy::derivable_impls)]
#![doc = "Utilities for hashing TLS environment warning payloads and generating stable labels."]

use crypto_suite::hashing::blake3;
use foundation_lazy::sync::Lazy;
use foundation_serialization::serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

type TlsEnvWarningTelemetryCallback =
    Arc<dyn Fn(&TlsEnvWarningTelemetryEvent) + Send + Sync + 'static>;

static TLS_ENV_WARNING_TELEMETRY_SINKS: Lazy<Mutex<Vec<TlsEnvWarningTelemetryCallback>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Delimiter used when hashing warning variable lists.
pub const VARIABLE_DELIMITER: u8 = 0x1f;

/// Compute the canonical fingerprint for the provided detail payload.
#[inline]
pub fn detail_fingerprint(detail: &str) -> i64 {
    fingerprint_from_bytes(detail.as_bytes())
}

/// Compute the canonical fingerprint for an iterator of TLS warning variables.
///
/// Empty iterators return `None` so callers can differentiate "no variables"
/// from a hashed payload.
pub fn variables_fingerprint<I, S>(variables: I) -> Option<i64>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut bytes = Vec::new();
    let mut first = true;
    for value in variables {
        let value = value.as_ref();
        if value.is_empty() {
            continue;
        }
        if first {
            first = false;
        } else {
            bytes.push(VARIABLE_DELIMITER);
        }
        bytes.extend_from_slice(value.as_bytes());
    }

    if first {
        None
    } else {
        Some(fingerprint_from_bytes(&bytes))
    }
}

/// Format the optional fingerprint as a lowercase hexadecimal label suitable
/// for Prometheus metrics.
#[inline]
pub fn fingerprint_label(fingerprint: Option<i64>) -> String {
    fingerprint
        .map(|value| {
            let unsigned = u64::from_le_bytes(value.to_le_bytes());
            format!("{unsigned:016x}")
        })
        .unwrap_or_else(|| "none".to_string())
}

/// Raw helper exposed so callers that already normalised byte payloads can
/// avoid re-allocating intermediate strings.
#[inline]
pub fn fingerprint_from_bytes(bytes: &[u8]) -> i64 {
    let digest = blake3::hash(bytes);
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&digest.as_bytes()[..8]);
    i64::from_le_bytes(buf)
}

/// Structured payload delivered to telemetry sinks whenever a TLS warning is
/// recorded.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct TlsEnvWarningTelemetryEvent {
    pub prefix: String,
    pub code: String,
    pub origin: WarningOrigin,
    pub total: u64,
    pub last_delta: u64,
    pub last_seen: u64,
    pub detail: Option<String>,
    pub detail_fingerprint: Option<i64>,
    pub detail_bucket: String,
    pub detail_changed: bool,
    pub variables: Vec<String>,
    pub variables_fingerprint: Option<i64>,
    pub variables_bucket: String,
    pub variables_changed: bool,
}

/// Guard returned by [`register_tls_env_warning_telemetry_sink`] that removes
/// the callback when dropped.
pub struct TlsEnvWarningTelemetrySinkGuard {
    sink: TlsEnvWarningTelemetryCallback,
}

impl Drop for TlsEnvWarningTelemetrySinkGuard {
    fn drop(&mut self) {
        if let Ok(mut sinks) = TLS_ENV_WARNING_TELEMETRY_SINKS.lock() {
            sinks.retain(|existing| !Arc::ptr_eq(existing, &self.sink));
        }
    }
}

/// Register a telemetry sink that will receive every
/// [`TlsEnvWarningTelemetryEvent`] emitted within the current process.
pub fn register_tls_env_warning_telemetry_sink<F>(sink: F) -> TlsEnvWarningTelemetrySinkGuard
where
    F: Fn(&TlsEnvWarningTelemetryEvent) + Send + Sync + 'static,
{
    let sink: TlsEnvWarningTelemetryCallback = Arc::new(sink);
    {
        let mut guard = TLS_ENV_WARNING_TELEMETRY_SINKS
            .lock()
            .expect("tls warning telemetry sinks");
        guard.push(Arc::clone(&sink));
    }
    TlsEnvWarningTelemetrySinkGuard { sink }
}

/// Dispatch the provided telemetry event to all registered sinks.
pub fn dispatch_tls_env_warning_event(event: &TlsEnvWarningTelemetryEvent) {
    let sinks = TLS_ENV_WARNING_TELEMETRY_SINKS
        .lock()
        .map(|guard| guard.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    for sink in sinks {
        sink(event);
    }
}

/// Clear all registered telemetry sinks. Intended for use in tests.
pub fn reset_tls_env_warning_telemetry_sinks_for_test() {
    if let Ok(mut sinks) = TLS_ENV_WARNING_TELEMETRY_SINKS.lock() {
        sinks.clear();
    }
}

/// Identifies the source that emitted the TLS environment warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
pub enum WarningOrigin {
    Diagnostics,
    PeerIngest,
}

impl WarningOrigin {
    /// Return the canonical label string for the origin.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            WarningOrigin::Diagnostics => "diagnostics",
            WarningOrigin::PeerIngest => "peer_ingest",
        }
    }

    /// Parse the origin from a canonical label string.
    pub fn from_str(label: &str) -> Option<Self> {
        match label {
            "diagnostics" => Some(WarningOrigin::Diagnostics),
            "peer_ingest" => Some(WarningOrigin::PeerIngest),
            _ => None,
        }
    }
}

impl Default for WarningOrigin {
    fn default() -> Self {
        WarningOrigin::PeerIngest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn detail_hash_matches_direct_bytes() {
        let payload = "identity requires TB_NODE_TLS_CERT";
        assert_eq!(
            detail_fingerprint(payload),
            fingerprint_from_bytes(payload.as_bytes())
        );
    }

    #[test]
    fn variables_hash_ignores_empty_values() {
        let fingerprint =
            variables_fingerprint(["TB_NODE_TLS_CERT", "TB_NODE_TLS_KEY", ""]).unwrap();
        let control = variables_fingerprint(["TB_NODE_TLS_CERT", "TB_NODE_TLS_KEY"]).unwrap();
        assert_eq!(fingerprint, control);
    }

    #[test]
    fn variables_hash_returns_none_for_empty_iter() {
        assert!(variables_fingerprint::<Vec<&str>, &str>(Vec::<&str>::new()).is_none());
    }

    #[test]
    fn fingerprint_label_formats_hex() {
        let value = detail_fingerprint("detail");
        let label = fingerprint_label(Some(value));
        let expected = format!("{:016x}", u64::from_le_bytes(value.to_le_bytes()));
        assert_eq!(label, expected);
        assert_eq!(fingerprint_label(None), "none");
    }

    #[test]
    fn origin_round_trips_from_label() {
        for origin in [WarningOrigin::Diagnostics, WarningOrigin::PeerIngest] {
            let label = origin.as_str();
            assert_eq!(WarningOrigin::from_str(label), Some(origin));
        }
        assert_eq!(WarningOrigin::from_str("unknown"), None);
    }

    #[test]
    fn telemetry_sink_receives_and_cleans_up() {
        reset_tls_env_warning_telemetry_sinks_for_test();

        let seen: Arc<Mutex<Vec<(String, String, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let seen_sink = Arc::clone(&seen);
            let _guard = register_tls_env_warning_telemetry_sink(move |event| {
                seen_sink.lock().expect("seen telemetry events").push((
                    event.prefix.clone(),
                    event.code.clone(),
                    event.total,
                ));
            });

            let detail_fp = detail_fingerprint("missing TB_NODE_TLS_KEY");
            let detail_bucket = fingerprint_label(Some(detail_fp));
            let variables_fp = variables_fingerprint(["TB_NODE_TLS_KEY"]);
            let variables_bucket = fingerprint_label(variables_fp);

            let event = TlsEnvWarningTelemetryEvent {
                prefix: "TB_NODE_TLS".to_string(),
                code: "missing_identity_component".to_string(),
                origin: WarningOrigin::Diagnostics,
                total: 1,
                last_delta: 1,
                last_seen: 5,
                detail: Some("missing TB_NODE_TLS_KEY".to_string()),
                detail_fingerprint: Some(detail_fp),
                detail_bucket,
                detail_changed: true,
                variables: vec!["TB_NODE_TLS_KEY".to_string()],
                variables_fingerprint: variables_fp,
                variables_bucket,
                variables_changed: true,
            };

            dispatch_tls_env_warning_event(&event);
        }

        assert_eq!(
            *seen.lock().expect("telemetry events recorded"),
            vec![(
                "TB_NODE_TLS".to_string(),
                "missing_identity_component".to_string(),
                1,
            )]
        );

        // After the guard is dropped, the sink should no longer receive events.
        let detail_fp = detail_fingerprint("missing TB_NODE_TLS_KEY");
        let detail_bucket = fingerprint_label(Some(detail_fp));
        let variables_fp = variables_fingerprint(["TB_NODE_TLS_KEY"]);
        let variables_bucket = fingerprint_label(variables_fp);

        let event = TlsEnvWarningTelemetryEvent {
            prefix: "TB_NODE_TLS".to_string(),
            code: "missing_identity_component".to_string(),
            origin: WarningOrigin::Diagnostics,
            total: 2,
            last_delta: 1,
            last_seen: 6,
            detail: Some("missing TB_NODE_TLS_KEY".to_string()),
            detail_fingerprint: Some(detail_fp),
            detail_bucket,
            detail_changed: false,
            variables: vec!["TB_NODE_TLS_KEY".to_string()],
            variables_fingerprint: variables_fp,
            variables_bucket,
            variables_changed: false,
        };

        dispatch_tls_env_warning_event(&event);

        assert_eq!(seen.lock().expect("telemetry events recorded").len(), 1);
    }
}
