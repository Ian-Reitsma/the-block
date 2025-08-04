//! Fee routing utilities.
//!
//! Implements selector-based fee decomposition as per CONSENSUS.md.

use pyo3::{create_exception, exceptions::PyException, prelude::*};
use thiserror::Error;

create_exception!(fee, ErrFeeOverflow, PyException);
create_exception!(fee, ErrInvalidSelector, PyException);

/// Maximum fee allowed before admission.
///
/// Defined by consensus – see `CONSENSUS.md` §"Fee Routing".
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
            let ct = fee128.div_ceil(2) as u64;
            let it = (fee128 / 2) as u64;
            Ok((ct, it))
        }
        _ => Err(FeeError::InvalidSelector),
    }
}

/// Python wrapper for [`decompose`].
#[pyfunction(name = "fee_decompose")]
pub fn decompose_py(selector: u8, fee: u64) -> PyResult<(u64, u64)> {
    decompose(selector, fee).map_err(|e| match e {
        FeeError::InvalidSelector => ErrInvalidSelector::new_err("invalid selector"),
        FeeError::Overflow => ErrFeeOverflow::new_err("fee overflow"),
    })
}
