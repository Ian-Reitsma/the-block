//! Fee routing utilities.
//!
//! Implements selector-based fee decomposition as per CONSENSUS.md.

use crate::py::{PyError, PyResult};
use std::fmt;

/// Maximum fee allowed before admission.
///
/// Defined by consensus – see `CONSENSUS.md` §"Fee Routing".
pub const MAX_FEE: u64 = (1u64 << 63) - 1;

/// Errors that can occur during fee decomposition.
#[derive(Debug, PartialEq)]
pub enum FeeError {
    InvalidSelector,
    Overflow,
}

impl fmt::Display for FeeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeeError::InvalidSelector => write!(f, "invalid selector"),
            FeeError::Overflow => write!(f, "fee overflow"),
        }
    }
}

impl std::error::Error for FeeError {}

/// Split a raw fee into consumer and industrial components based on a
/// consumer percentage.
///
/// `pct_ct` expresses the fraction of `fee` paid in consumer tokens. It must be
/// within `0..=100`, where `0` routes the entire fee to industrial tokens and
/// `100` routes it entirely to consumer tokens.
///
/// Returns `(fee_ct, fee_it)` on success.
pub fn decompose(pct_ct: u8, fee: u64) -> Result<(u64, u64), FeeError> {
    if fee > MAX_FEE {
        return Err(FeeError::Overflow);
    }
    if pct_ct > 100 {
        return Err(FeeError::InvalidSelector);
    }
    let fee128 = fee as u128;
    let pct128 = pct_ct as u128;
    let ct = (fee128 * pct128).div_ceil(100) as u64;
    let it = fee - ct;
    Ok((ct, it))
}

/// Python wrapper for [`decompose`].
pub fn decompose_py(pct_ct: u8, fee: u64) -> PyResult<(u64, u64)> {
    decompose(pct_ct, fee).map_err(|e| match e {
        FeeError::InvalidSelector => PyError::value("invalid selector"),
        FeeError::Overflow => PyError::value("fee overflow"),
    })
}

#[derive(Debug, Clone, Copy)]
pub struct ErrInvalidSelector;

impl ErrInvalidSelector {
    pub fn new_err(msg: impl Into<String>) -> PyError {
        PyError::value(msg)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ErrFeeOverflow;

impl ErrFeeOverflow {
    pub fn new_err(msg: impl Into<String>) -> PyError {
        PyError::value(msg)
    }
}
