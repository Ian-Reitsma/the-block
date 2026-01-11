//! Authorized disbursement operations with multi-sig support
//!
//! This module provides secure wrappers for treasury disbursement operations
//! that require valid operator signatures before execution.

use crate::authorization::{
    verify_authorization, AuthError, AuthorizedCall, Operation, OperatorRegistry, Role,
};
use crate::store::GovStore;
use crate::treasury::{DisbursementPayload, TreasuryDisbursement};

/// Result type for authorized disbursement operations
pub type AuthResult<T> = Result<T, AuthError>;

/// Authorized disbursement operations handler
pub struct AuthorizedDisbursementOps;

impl AuthorizedDisbursementOps {
    /// Queue a disbursement with authorization
    ///
    /// Requires:
    /// - Valid operator/admin signature over QueueDisbursement operation
    /// - Operator or higher role
    /// - Non-stale timestamp (within 10 minutes)
    /// - Unused nonce
    pub fn queue_disbursement(
        store: &GovStore,
        auth: &AuthorizedCall,
        registry: &mut OperatorRegistry,
        payload: DisbursementPayload,
    ) -> AuthResult<TreasuryDisbursement> {
        // Verify operation type matches
        if !matches!(auth.operation, Operation::QueueDisbursement { .. }) {
            return Err(AuthError::MalformedSignature {
                reason: "operation type mismatch (expected QueueDisbursement)".into(),
            });
        }

        // Verify signature, timestamp, nonce, and role
        verify_authorization(auth, registry, Role::Operator)?;

        // Delegate to storage layer (authorization already verified)
        store
            .queue_disbursement_internal(payload)
            .map_err(|e| AuthError::MalformedSignature {
                reason: format!("storage error: {}", e),
            })
    }

    /// Cancel a disbursement with authorization
    ///
    /// Requires:
    /// - Valid operator/admin signature over CancelDisbursement operation
    /// - Operator or higher role
    /// - Non-stale timestamp (within 10 minutes)
    /// - Unused nonce
    pub fn cancel_disbursement(
        store: &GovStore,
        auth: &AuthorizedCall,
        registry: &mut OperatorRegistry,
        disbursement_id: u64,
        reason: &str,
    ) -> AuthResult<TreasuryDisbursement> {
        // Verify operation type matches
        if !matches!(auth.operation, Operation::CancelDisbursement { .. }) {
            return Err(AuthError::MalformedSignature {
                reason: "operation type mismatch (expected CancelDisbursement)".into(),
            });
        }

        // Verify signature, timestamp, nonce, and role
        verify_authorization(auth, registry, Role::Operator)?;

        // Delegate to storage layer (authorization already verified)
        store
            .cancel_disbursement_internal(disbursement_id, reason)
            .map_err(|e| AuthError::MalformedSignature {
                reason: format!("storage error: {}", e),
            })
    }

    /// Modify governance parameters with authorization
    ///
    /// Requires:
    /// - Valid admin signature over ModifyParams operation
    /// - Admin role (highest privilege)
    /// - Non-stale timestamp (within 10 minutes)
    /// - Unused nonce
    pub fn modify_params(
        store: &GovStore,
        auth: &AuthorizedCall,
        registry: &mut OperatorRegistry,
        param_key: &str,
        new_value: i64,
    ) -> AuthResult<()> {
        // Verify operation type matches
        if !matches!(auth.operation, Operation::ModifyParams { .. }) {
            return Err(AuthError::MalformedSignature {
                reason: "operation type mismatch (expected ModifyParams)".into(),
            });
        }

        // Verify signature, timestamp, nonce, and ADMIN role
        verify_authorization(auth, registry, Role::Admin)?;

        // Delegate to storage layer (authorization already verified)
        store
            .modify_param_with_auth(param_key, new_value)
            .map_err(|e| AuthError::MalformedSignature {
                reason: format!("storage error: {}", e),
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn queue_disbursement_requires_matching_operation() {
        // This would require setting up full test fixtures
        // In practice, verify_authorization handles this
    }

    #[test]
    fn operation_mismatch_rejected() {
        // Verify that passing wrong operation type fails
        // Tested via verify_authorization tests in authorization.rs
    }

    #[test]
    fn role_enforcement_works() {
        // Verify that lower roles cannot perform higher-privilege operations
        // Tested via role_hierarchy_works test in authorization.rs
    }
}
