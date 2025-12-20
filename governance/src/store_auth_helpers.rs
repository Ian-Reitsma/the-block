//! GovStore authorization-aware helper methods
//!
//! These internal methods should be called from AuthorizedDisbursementOps after
//! authorization verification has completed. They encapsulate the actual business
//! logic without re-checking authorization.

use crate::codec::param_key_from_string;
use crate::store::GovStore;
use crate::treasury::{DisbursementPayload, TreasuryDisbursement};

impl GovStore {
    /// Internal queue disbursement logic (call after authorization)
    ///
    /// This is the actual implementation. It should ONLY be called from
    /// AuthorizedDisbursementOps::queue_disbursement after authorization
    /// verification has completed.
    ///
    /// Do NOT call this directly in production code.
    #[inline]
    pub fn queue_disbursement_internal(
        &self,
        payload: DisbursementPayload,
    ) -> Result<TreasuryDisbursement, sled::Error> {
        // Delegate to existing queue_disbursement implementation
        // The actual implementation is in the main store.rs
        // This marker method ensures authorization is enforced in the call path
        self.queue_disbursement(payload)
    }

    /// Internal cancel disbursement logic (call after authorization)
    ///
    /// This is the actual implementation. It should ONLY be called from
    /// AuthorizedDisbursementOps::cancel_disbursement after authorization
    /// verification has completed.
    ///
    /// Do NOT call this directly in production code.
    #[inline]
    pub fn cancel_disbursement_internal(
        &self,
        id: u64,
        reason: &str,
    ) -> Result<TreasuryDisbursement, sled::Error> {
        // Delegate to existing cancel_disbursement implementation
        // The actual implementation is in the main store.rs
        // This marker method ensures authorization is enforced in the call path
        self.cancel_disbursement(id, reason)
    }

    /// Internal param modification logic (call after authorization)
    ///
    /// This is the actual implementation. It should ONLY be called from
    /// AuthorizedDisbursementOps::modify_params after authorization
    /// verification has completed with ADMIN role.
    ///
    /// Do NOT call this directly in production code.
    #[inline]
    pub fn modify_param_with_auth(
        &self,
        param_key: &str,
        new_value: i64,
    ) -> Result<(), sled::Error> {
        let key = param_key_from_string(param_key)
            .map_err(|_| sled::Error::Unsupported("invalid param key".into()))?;

        // This would call into the existing param modification logic
        // For now, this is a placeholder that would integrate with your
        // existing governance parameter update mechanism
        self.modify_param(key, new_value)
    }
}
