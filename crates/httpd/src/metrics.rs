//! Shared helpers for emitting first-party telemetry payloads.
//!
//! The node, CLI, and metrics aggregator all expose textual metric snapshots.
//! The logic is simple but duplicated, so we provide a small utility here that
//! keeps content negotiation consistent and centralises the text response
//! format.

use crate::{Response, StatusCode};
use runtime::telemetry::Registry;

/// Render the supplied telemetry registry as a text response understood by the
/// first-party monitoring stack.
#[must_use]
pub fn telemetry_snapshot(registry: &Registry) -> Response {
    match registry.render_bytes() {
        Ok(body) => Response::new(StatusCode::OK)
            .with_header("content-type", runtime::telemetry::TEXT_MIME)
            .with_body(body),
        Err(err) => Response::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_header("content-type", runtime::telemetry::TEXT_MIME)
            .with_body(format!("telemetry export failed: {err}").into_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::telemetry_snapshot;
    use runtime::telemetry::Registry;

    #[test]
    fn renders_text_payload() {
        let registry = Registry::new();
        let counter = registry
            .register_counter("test_counter_total", "counter for testing")
            .expect("register counter");
        counter.inc();
        let response = telemetry_snapshot(&registry);
        assert_eq!(response.status(), crate::StatusCode::OK);
        assert_eq!(
            response.header("content-type"),
            Some(runtime::telemetry::TEXT_MIME)
        );
        let body = String::from_utf8(response.body().to_vec()).expect("utf8 body");
        assert!(body.contains("test_counter_total"));
    }
}
