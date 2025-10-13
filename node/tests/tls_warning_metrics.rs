#![cfg(feature = "telemetry")]

use crypto_suite::hashing::blake3;
use diagnostics;
use http_env::server_tls_from_env;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tls_warning::WarningOrigin;

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("monotonic clock")
        .as_nanos()
}

#[test]
fn tls_env_warning_metrics_increment_on_log() {
    the_block::telemetry::reset_tls_env_warning_forwarder_for_testing();
    the_block::telemetry::install_tls_env_warning_forwarder();
    the_block::telemetry::clear_tls_env_warning_snapshots_for_testing();

    let prefix = format!("TB_TEST_NODE_TLS_{}", unique_suffix());
    let counter = the_block::telemetry::TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    counter.reset();
    let events_counter = the_block::telemetry::TLS_ENV_WARNING_EVENTS_TOTAL
        .ensure_handle_for_label_values(&[
            prefix.as_str(),
            "missing_identity_component",
            WarningOrigin::Diagnostics.as_str(),
        ])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    events_counter.reset();
    let gauge = the_block::telemetry::TLS_ENV_WARNING_LAST_SEEN_SECONDS
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    gauge.set(0);
    let detail_fingerprint = the_block::telemetry::TLS_ENV_WARNING_DETAIL_FINGERPRINT
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    detail_fingerprint.set(0);
    let variables_fingerprint = the_block::telemetry::TLS_ENV_WARNING_VARIABLES_FINGERPRINT
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    variables_fingerprint.set(0);

    let cert_var = format!("{prefix}_CERT");
    let key_var = format!("{prefix}_KEY");
    let client_ca_var = format!("{prefix}_CLIENT_CA");
    let client_ca_optional_var = format!("{prefix}_CLIENT_CA_OPTIONAL");

    std::env::set_var(&cert_var, "/tmp/test-node-cert.pem");
    std::env::remove_var(&key_var);
    std::env::remove_var(&client_ca_var);
    std::env::remove_var(&client_ca_optional_var);

    let result = server_tls_from_env(&prefix, None);
    assert!(result.is_err());

    assert_eq!(counter.value(), 1);
    assert!(gauge.get() > 0);
    assert_eq!(events_counter.value(), 1);
    let expected_detail =
        format!("identity requires both {cert_var} and {key_var}; missing {key_var}");
    let mut detail_bytes = [0u8; 8];
    detail_bytes.copy_from_slice(&blake3::hash(expected_detail.as_bytes()).as_bytes()[..8]);
    let expected_detail_fingerprint = i64::from_le_bytes(detail_bytes);
    assert_eq!(detail_fingerprint.get(), expected_detail_fingerprint);
    let expected_variables = vec![key_var.clone()];
    let mut fingerprint_bytes = Vec::new();
    for (idx, value) in expected_variables.iter().enumerate() {
        if idx > 0 {
            fingerprint_bytes.push(0x1f);
        }
        fingerprint_bytes.extend_from_slice(value.as_bytes());
    }
    let mut variables_bytes = [0u8; 8];
    variables_bytes.copy_from_slice(&blake3::hash(&fingerprint_bytes).as_bytes()[..8]);
    let expected_variables_fingerprint = i64::from_le_bytes(variables_bytes);
    assert_eq!(variables_fingerprint.get(), expected_variables_fingerprint);

    let snapshots = the_block::telemetry::tls_env_warning_snapshots();
    let snapshot = snapshots
        .iter()
        .find(|entry| entry.prefix == prefix && entry.code == "missing_identity_component")
        .expect("warning snapshot recorded");
    assert_eq!(snapshot.total, 1);
    assert_eq!(snapshot.last_delta, 1);
    assert!(snapshot.last_seen > 0);
    assert_eq!(snapshot.origin, WarningOrigin::Diagnostics);
    assert!(snapshot
        .detail
        .as_ref()
        .is_some_and(|detail| detail.contains(&key_var)));
    assert_eq!(
        snapshot.detail_fingerprint,
        Some(expected_detail_fingerprint)
    );
    assert_eq!(snapshot.variables, vec![key_var.clone()]);
    assert_eq!(
        snapshot.variables_fingerprint,
        Some(expected_variables_fingerprint)
    );
    let detail_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_detail_fingerprint.to_le_bytes())
    );
    assert_eq!(
        snapshot
            .detail_fingerprint_counts
            .get(&detail_bucket)
            .copied(),
        Some(1)
    );
    let variables_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_variables_fingerprint.to_le_bytes())
    );
    assert_eq!(
        snapshot
            .variables_fingerprint_counts
            .get(&variables_bucket)
            .copied(),
        Some(1)
    );

    std::env::remove_var(&cert_var);
    std::env::remove_var(&key_var);
    std::env::remove_var(&client_ca_var);
    std::env::remove_var(&client_ca_optional_var);
}

#[test]
fn tls_env_warning_metrics_from_diagnostics_without_sink() {
    the_block::telemetry::reset_tls_env_warning_forwarder_for_testing();
    the_block::telemetry::ensure_tls_env_warning_diagnostics_bridge();
    the_block::telemetry::clear_tls_env_warning_snapshots_for_testing();

    let prefix = format!("TB_TEST_DIAG_TLS_{}", unique_suffix());
    let code = "manual_warning";
    let detail = format!("manual diagnostics warning for {prefix}");
    let variables = vec![format!("{prefix}_CERT")];

    let counter = the_block::telemetry::TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&[prefix.as_str(), code])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    counter.reset();
    let events_counter = the_block::telemetry::TLS_ENV_WARNING_EVENTS_TOTAL
        .ensure_handle_for_label_values(&[
            prefix.as_str(),
            code,
            WarningOrigin::Diagnostics.as_str(),
        ])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    events_counter.reset();
    let gauge = the_block::telemetry::TLS_ENV_WARNING_LAST_SEEN_SECONDS
        .ensure_handle_for_label_values(&[prefix.as_str(), code])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    gauge.set(0);
    let detail_fingerprint = the_block::telemetry::TLS_ENV_WARNING_DETAIL_FINGERPRINT
        .ensure_handle_for_label_values(&[prefix.as_str(), code])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    detail_fingerprint.set(0);
    let variables_fingerprint = the_block::telemetry::TLS_ENV_WARNING_VARIABLES_FINGERPRINT
        .ensure_handle_for_label_values(&[prefix.as_str(), code])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    variables_fingerprint.set(0);

    assert!(!http_env::has_tls_warning_sinks());

    diagnostics::warn!(
        target: "http_env.tls_env",
        prefix = %prefix,
        code = code,
        detail = %detail,
        variables = ?variables,
        "tls_env_warning"
    );

    assert_eq!(counter.value(), 1);
    assert!(gauge.get() > 0);
    assert_eq!(events_counter.value(), 1);

    let mut detail_bytes = [0u8; 8];
    detail_bytes.copy_from_slice(&blake3::hash(detail.as_bytes()).as_bytes()[..8]);
    let expected_detail_fingerprint = i64::from_le_bytes(detail_bytes);
    assert_eq!(detail_fingerprint.get(), expected_detail_fingerprint);

    let mut fingerprint_bytes = Vec::new();
    for (idx, value) in variables.iter().enumerate() {
        if idx > 0 {
            fingerprint_bytes.push(0x1f);
        }
        fingerprint_bytes.extend_from_slice(value.as_bytes());
    }
    let mut variables_bytes = [0u8; 8];
    variables_bytes.copy_from_slice(&blake3::hash(&fingerprint_bytes).as_bytes()[..8]);
    let expected_variables_fingerprint = i64::from_le_bytes(variables_bytes);
    assert_eq!(variables_fingerprint.get(), expected_variables_fingerprint);

    let snapshots = the_block::telemetry::tls_env_warning_snapshots();
    let snapshot = snapshots
        .iter()
        .find(|entry| entry.prefix == prefix && entry.code == code)
        .expect("warning snapshot recorded");
    assert_eq!(snapshot.total, 1);
    assert_eq!(snapshot.last_delta, 1);
    assert!(snapshot.last_seen > 0);
    assert_eq!(snapshot.origin, WarningOrigin::Diagnostics);
    assert_eq!(snapshot.detail, Some(detail));
    assert_eq!(snapshot.variables, variables);
    assert_eq!(
        snapshot.detail_fingerprint,
        Some(expected_detail_fingerprint)
    );
    assert_eq!(
        snapshot.variables_fingerprint,
        Some(expected_variables_fingerprint)
    );

    let detail_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_detail_fingerprint.to_le_bytes())
    );
    assert_eq!(
        snapshot
            .detail_fingerprint_counts
            .get(&detail_bucket)
            .copied(),
        Some(1)
    );
    let variables_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_variables_fingerprint.to_le_bytes())
    );
    assert_eq!(
        snapshot
            .variables_fingerprint_counts
            .get(&variables_bucket)
            .copied(),
        Some(1)
    );
}

#[test]
fn tls_env_warning_telemetry_sink_captures_forwarder_events() {
    the_block::telemetry::reset_tls_env_warning_forwarder_for_testing();
    the_block::telemetry::clear_tls_env_warning_snapshots_for_testing();
    the_block::telemetry::install_tls_env_warning_forwarder();

    let events = Arc::new(Mutex::new(Vec::<
        the_block::telemetry::TlsEnvWarningTelemetryEvent,
    >::new()));
    let guard = the_block::telemetry::register_tls_env_warning_telemetry_sink({
        let events = Arc::clone(&events);
        move |event| {
            events.lock().expect("events lock").push(event.clone());
        }
    });

    let prefix = format!("TB_TEST_SINK_TLS_{}", unique_suffix());
    let counter = the_block::telemetry::TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    counter.reset();

    let cert_var = format!("{prefix}_CERT");
    let key_var = format!("{prefix}_KEY");
    let client_ca_var = format!("{prefix}_CLIENT_CA");
    let client_ca_optional_var = format!("{prefix}_CLIENT_CA_OPTIONAL");

    std::env::set_var(&cert_var, "/tmp/test-node-cert.pem");
    std::env::remove_var(&key_var);
    std::env::remove_var(&client_ca_var);
    std::env::remove_var(&client_ca_optional_var);

    let result = server_tls_from_env(&prefix, None);
    assert!(result.is_err());

    std::env::remove_var(&cert_var);
    std::env::remove_var(&key_var);
    std::env::remove_var(&client_ca_var);
    std::env::remove_var(&client_ca_optional_var);

    drop(guard);

    let events = events.lock().expect("events lock");
    assert_eq!(events.len(), 1, "expected a single telemetry sink event");
    let event = events.first().expect("telemetry event");
    assert_eq!(event.prefix, prefix);
    assert_eq!(event.code, "missing_identity_component");
    assert_eq!(event.origin, WarningOrigin::Diagnostics);
    assert_eq!(event.total, 1);
    assert_eq!(event.last_delta, 1);
    assert!(event.last_seen > 0);
    assert!(event.detail_changed);
    assert!(event.variables_changed);
    assert!(event
        .detail
        .as_ref()
        .is_some_and(|value| value.contains(&key_var)));
    let expected_detail =
        format!("identity requires both {prefix}_CERT and {prefix}_KEY; missing {prefix}_KEY");
    let mut detail_bytes = [0u8; 8];
    detail_bytes.copy_from_slice(&blake3::hash(expected_detail.as_bytes()).as_bytes()[..8]);
    let expected_detail_fingerprint = i64::from_le_bytes(detail_bytes);
    assert_eq!(event.detail_fingerprint, Some(expected_detail_fingerprint));
    let expected_detail_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_detail_fingerprint.to_le_bytes())
    );
    assert_eq!(event.detail_bucket, expected_detail_bucket);
    assert_eq!(event.variables, vec![key_var.clone()]);
    let fingerprint_bytes = key_var.as_bytes();
    let mut variables_bytes = [0u8; 8];
    variables_bytes.copy_from_slice(&blake3::hash(fingerprint_bytes).as_bytes()[..8]);
    let expected_variables_fingerprint = i64::from_le_bytes(variables_bytes);
    assert_eq!(
        event.variables_fingerprint,
        Some(expected_variables_fingerprint)
    );
    let expected_variables_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_variables_fingerprint.to_le_bytes())
    );
    assert_eq!(event.variables_bucket, expected_variables_bucket);
}

#[test]
fn tls_env_warning_telemetry_sink_captures_diagnostics_bridge_events() {
    the_block::telemetry::reset_tls_env_warning_forwarder_for_testing();
    the_block::telemetry::ensure_tls_env_warning_diagnostics_bridge();
    the_block::telemetry::clear_tls_env_warning_snapshots_for_testing();

    let events = Arc::new(Mutex::new(Vec::<
        the_block::telemetry::TlsEnvWarningTelemetryEvent,
    >::new()));
    let guard = the_block::telemetry::register_tls_env_warning_telemetry_sink({
        let events = Arc::clone(&events);
        move |event| {
            events.lock().expect("events lock").push(event.clone());
        }
    });

    assert!(!http_env::has_tls_warning_sinks());

    let prefix = format!("TB_TEST_DIAG_TLS_{}", unique_suffix());
    let code = "manual_warning";
    let detail = format!("manual diagnostics warning for {prefix}");
    let variables = vec![format!("{prefix}_CERT")];

    diagnostics::warn!(
        target: "http_env.tls_env",
        prefix = %prefix,
        code = code,
        detail = %detail,
        variables = ?variables,
        "tls_env_warning"
    );

    drop(guard);

    let events = events.lock().expect("events lock");
    assert_eq!(
        events.len(),
        1,
        "expected a single telemetry event from diagnostics bridge"
    );
    let event = events.first().expect("telemetry event");
    assert_eq!(event.prefix, prefix);
    assert_eq!(event.code, code);
    assert_eq!(event.origin, WarningOrigin::Diagnostics);
    assert_eq!(event.total, 1);
    assert_eq!(event.last_delta, 1);
    assert!(event.last_seen > 0);
    assert!(event.detail_changed);
    assert!(event.variables_changed);
    assert_eq!(event.detail.as_deref(), Some(detail.as_str()));
    let mut detail_bytes = [0u8; 8];
    detail_bytes.copy_from_slice(&blake3::hash(detail.as_bytes()).as_bytes()[..8]);
    let expected_detail_fingerprint = i64::from_le_bytes(detail_bytes);
    assert_eq!(event.detail_fingerprint, Some(expected_detail_fingerprint));
    let expected_detail_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_detail_fingerprint.to_le_bytes())
    );
    assert_eq!(event.detail_bucket, expected_detail_bucket);
    assert_eq!(event.variables, variables);
    let mut fingerprint_bytes = Vec::new();
    fingerprint_bytes.extend_from_slice(event.variables[0].as_bytes());
    let mut variables_bytes = [0u8; 8];
    variables_bytes.copy_from_slice(&blake3::hash(&fingerprint_bytes).as_bytes()[..8]);
    let expected_variables_fingerprint = i64::from_le_bytes(variables_bytes);
    assert_eq!(
        event.variables_fingerprint,
        Some(expected_variables_fingerprint)
    );
    let expected_variables_bucket = format!(
        "{:016x}",
        u64::from_le_bytes(expected_variables_fingerprint.to_le_bytes())
    );
    assert_eq!(event.variables_bucket, expected_variables_bucket);
}
