//! Test helpers that provide approximate equality assertions without relying on
//! third-party crates. These helpers are intentionally lightweight and avoid
//! floating-point surprises by combining relative and absolute tolerances.

/// Assert that two floating-point values are approximately equal using the
/// default tolerance of `1e-12` scaled by the magnitude of the inputs.
pub fn assert_close(actual: f64, expected: f64) {
    assert_close_with(actual, expected, 1e-12);
}

/// Assert that two floating-point values are approximately equal using the
/// provided tolerance. The tolerance acts as a relative threshold scaled by the
/// larger magnitude of the operands while also providing an absolute floor so
/// comparisons near zero remain meaningful.
pub fn assert_close_with(actual: f64, expected: f64, epsilon: f64) {
    let diff = (actual - expected).abs();
    let scale = actual.abs().max(expected.abs()).max(1.0);
    let limit = epsilon * scale;
    if diff > limit {
        panic!(
            "expected values to be close: left={actual}, right={expected}, diff={diff}, tolerance={limit}"
        );
    }
}
