#![cfg(feature = "telemetry")]

use http_env::server_tls_from_env;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("monotonic clock")
        .as_nanos()
}

#[test]
fn tls_env_warning_metrics_increment_on_log() {
    the_block::telemetry::install_tls_env_warning_forwarder();

    let prefix = format!("TB_TEST_NODE_TLS_{}", unique_suffix());
    let counter = the_block::telemetry::TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    counter.reset();
    let gauge = the_block::telemetry::TLS_ENV_WARNING_LAST_SEEN_SECONDS
        .ensure_handle_for_label_values(&[prefix.as_str(), "missing_identity_component"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    gauge.set(0);

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

    std::env::remove_var(cert_var);
    std::env::remove_var(key_var);
    std::env::remove_var(client_ca_var);
    std::env::remove_var(client_ca_optional_var);
}
