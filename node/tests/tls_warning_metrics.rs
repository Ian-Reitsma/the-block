#![cfg(feature = "telemetry")]

use diagnostics::warn;

#[test]
fn tls_env_warning_metrics_increment_on_log() {
    the_block::telemetry::install_tls_env_warning_forwarder();

    let handle = the_block::telemetry::TLS_ENV_WARNING_TOTAL
        .ensure_handle_for_label_values(&["TB_NODE_TLS", "missing_cert"])
        .expect(the_block::telemetry::LABEL_REGISTRATION_ERR);
    handle.reset();

    warn!(
        target: "http_env.tls_env",
        prefix = %"TB_NODE_TLS",
        code = "missing_cert",
        detail = %"certificate not present",
        variables = ?vec!["TB_NODE_TLS_CERT"],
        "tls_env_warning"
    );

    assert_eq!(handle.value(), 1);
}
