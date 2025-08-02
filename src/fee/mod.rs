//! Fee routing utilities.
//!
//! Implements selector-based fee decomposition as per CONSENSUS.md.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use thiserror::Error;

/// Maximum fee allowed before admission.
pub const MAX_FEE: u64 = (1u64 << 63) - 1;

/// Errors that can occur during fee decomposition.
#[derive(Debug, Error, PartialEq)]
pub enum FeeError {
    #[error("invalid selector")]
    InvalidSelector,
    #[error("fee overflow")]
    Overflow,
}

/// Split a raw fee into consumer and industrial components.
///
/// Returns `(fee_ct, fee_it)` on success.
pub fn decompose(selector: u8, fee: u64) -> Result<(u64, u64), FeeError> {
    if fee > MAX_FEE {
        return Err(FeeError::Overflow);
    }
    match selector {
        0 => Ok((fee, 0)),
        1 => Ok((0, fee)),
        2 => {
            let fee128 = fee as u128;
            let ct = ((fee128 + 1) / 2) as u64;
            let it = (fee128 / 2) as u64;
            Ok((ct, it))
        }
        _ => Err(FeeError::InvalidSelector),
    }
}

/// Python wrapper for [`decompose`].
#[pyfunction(name = "fee_decompose")]
pub fn decompose_py(selector: u8, fee: u64) -> PyResult<(u64, u64)> {
    decompose(selector, fee).map_err(|e| PyValueError::new_err(e.to_string()))
}
