#![allow(clippy::unwrap_used, clippy::expect_used)]

use the_block::fee::{decompose, FeeError, MAX_FEE};

#[test]
fn splits_selector_cases() {
    assert_eq!(decompose(0, 10).unwrap(), (10, 0));
    assert_eq!(decompose(1, 5).unwrap(), (0, 5));
    assert_eq!(decompose(2, 5).unwrap(), (3, 2));
}

#[test]
fn rejects_overflow_and_selector() {
    assert_eq!(decompose(3, 1).unwrap_err(), FeeError::InvalidSelector);
    assert_eq!(decompose(0, MAX_FEE + 1).unwrap_err(), FeeError::Overflow);
}
