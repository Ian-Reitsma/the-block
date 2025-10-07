//! Shared helpers for emitting Prometheus metrics payloads.
//!
//! The node, CLI, and metrics aggregator all expose Prometheus endpoints. The
//! logic is simple but duplicated, so we provide a small utility here that keeps
//! content negotiation consistent and centralises the text response format.

use crate::{Response, StatusCode};
use runtime::telemetry::Registry;

/// Render the supplied telemetry registry as a Prometheus text response.
#[must_use]
pub fn prometheus(registry: &Registry) -> Response {
    Response::new(StatusCode::OK)
        .with_header("content-type", "text/plain; version=0.0.4")
        .with_body(registry.render().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::prometheus;
    use runtime::telemetry::Registry;

    #[test]
    fn renders_prometheus_payload() {
        let registry = Registry::new();
        let counter = registry
            .register_counter("test_counter_total", "counter for testing")
            .expect("register counter");
        counter.inc();
        let response = prometheus(&registry);
        assert_eq!(response.status(), crate::StatusCode::OK);
        assert_eq!(
            response.header("content-type"),
            Some("text/plain; version=0.0.4")
        );
        let body = String::from_utf8(response.body().to_vec()).expect("utf8 body");
        assert!(body.contains("test_counter_total"));
    }
}
