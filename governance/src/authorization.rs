//! Authorization system for governance operations
//!
//! Implements multi-signature authorization for sensitive treasury operations.
//! Prevents unauthorized access to circuit breaker controls and disbursement
//! queue manipulation.

use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};

#[cfg(test)]
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum age for signed operations (10 minutes)
const MAX_OPERATION_AGE_SECS: u64 = 600;

/// Governance operation types requiring authorization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Operation {
    /// Queue a treasury disbursement
    QueueDisbursement { proposal_id: String, amount_ct: u64 },
    /// Cancel a pending disbursement
    CancelDisbursement { disbursement_id: String },
    /// Force circuit breaker open
    ForceCircuitOpen,
    /// Force circuit breaker closed
    ForceCircuitClosed,
    /// Reset circuit breaker counters
    ResetCircuitBreaker,
    /// Modify governance parameters
    ModifyParams { param_key: String },
}

impl Operation {
    /// Serialize operation for signing
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();

        match self {
            Operation::QueueDisbursement {
                proposal_id,
                amount_ct,
            } => {
                hasher.update(b"queue_disbursement");
                hasher.update(proposal_id.as_bytes());
                hasher.update(&amount_ct.to_le_bytes());
            }
            Operation::CancelDisbursement { disbursement_id } => {
                hasher.update(b"cancel_disbursement");
                hasher.update(disbursement_id.as_bytes());
            }
            Operation::ForceCircuitOpen => {
                hasher.update(b"force_circuit_open");
            }
            Operation::ForceCircuitClosed => {
                hasher.update(b"force_circuit_closed");
            }
            Operation::ResetCircuitBreaker => {
                hasher.update(b"reset_circuit_breaker");
            }
            Operation::ModifyParams { param_key } => {
                hasher.update(b"modify_params");
                hasher.update(param_key.as_bytes());
            }
        }

        hasher.finalize().as_bytes().to_vec()
    }
}

/// Authorized call with signature
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AuthorizedCall {
    pub operation: Operation,
    pub timestamp: u64,
    pub nonce: u64,
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Vec<u8>,
    pub operator_id: String,
}

impl AuthorizedCall {
    /// Build signing preimage
    pub fn signing_message(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.operation.to_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.update(self.operator_id.as_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}

/// Operator role in governance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Role {
    /// Can execute queued disbursements
    Executor,
    /// Can queue disbursements and control circuit breaker
    Operator,
    /// Can modify roles and upgrade system
    Admin,
}

/// Registered operator with public key and role
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct Operator {
    pub operator_id: String,
    pub verifying_key: VerifyingKey,
    pub role: Role,
    pub registered_at: u64,
}

/// Authorization errors
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum AuthError {
    InvalidSignature {
        operator_id: String,
    },
    UnknownOperator {
        operator_id: String,
    },
    NonceReused {
        operator_id: String,
        nonce: u64,
    },
    OperationExpired {
        timestamp: u64,
        current: u64,
    },
    InsufficientPermissions {
        operator_id: String,
        required: Role,
        actual: Role,
    },
    MalformedSignature {
        reason: String,
    },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature { operator_id } => {
                write!(f, "Invalid signature from operator {}", operator_id)
            }
            Self::UnknownOperator { operator_id } => {
                write!(f, "Unknown operator: {}", operator_id)
            }
            Self::NonceReused { operator_id, nonce } => {
                write!(f, "Nonce {} reused by operator {}", nonce, operator_id)
            }
            Self::OperationExpired { timestamp, current } => {
                write!(
                    f,
                    "Operation expired: timestamp {} < current {}",
                    timestamp, current
                )
            }
            Self::InsufficientPermissions {
                operator_id,
                required,
                actual,
            } => {
                write!(
                    f,
                    "Operator {} has role {:?} but {:?} required",
                    operator_id, actual, required
                )
            }
            Self::MalformedSignature { reason } => {
                write!(f, "Malformed signature: {}", reason)
            }
        }
    }
}

impl std::error::Error for AuthError {}

/// Operator registry with nonce tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct OperatorRegistry {
    operators: HashMap<String, Operator>,
    used_nonces: HashMap<(String, u64), u64>,
}

impl OperatorRegistry {
    pub fn new() -> Self {
        Self {
            operators: HashMap::new(),
            used_nonces: HashMap::new(),
        }
    }

    /// Register a new operator
    pub fn register_operator(
        &mut self,
        operator_id: String,
        verifying_key: VerifyingKey,
        role: Role,
    ) -> Result<(), String> {
        if operator_id.is_empty() {
            return Err("operator_id cannot be empty".into());
        }
        if operator_id.len() > 256 {
            return Err("operator_id too long".into());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.operators.insert(
            operator_id.clone(),
            Operator {
                operator_id,
                verifying_key,
                role,
                registered_at: timestamp,
            },
        );
        Ok(())
    }

    /// Get operator by ID
    pub fn get_operator(&self, operator_id: &str) -> Option<&Operator> {
        self.operators.get(operator_id)
    }

    /// Check if operator has required role
    pub fn has_role(&self, operator_id: &str, required_role: Role) -> bool {
        if let Some(op) = self.operators.get(operator_id) {
            match (required_role, op.role) {
                (Role::Executor, _) => true, // All roles can execute
                (Role::Operator, Role::Operator) | (Role::Operator, Role::Admin) => true,
                (Role::Admin, Role::Admin) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    /// Check and record nonce
    fn check_nonce(
        &mut self,
        operator_id: &str,
        nonce: u64,
        timestamp: u64,
    ) -> Result<(), AuthError> {
        let key = (operator_id.to_string(), nonce);
        if self.used_nonces.contains_key(&key) {
            return Err(AuthError::NonceReused {
                operator_id: operator_id.to_string(),
                nonce,
            });
        }
        self.used_nonces.insert(key, timestamp);
        Ok(())
    }

    /// Prune old nonces (older than 24 hours)
    pub fn prune_old_nonces(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now.saturating_sub(86400);
        self.used_nonces.retain(|_, timestamp| *timestamp >= cutoff);
    }

    /// List all operators
    pub fn list_operators(&self) -> Vec<&Operator> {
        self.operators.values().collect()
    }
}

/// Verify authorized call signature and permissions
pub fn verify_authorization(
    call: &AuthorizedCall,
    registry: &mut OperatorRegistry,
    required_role: Role,
) -> Result<(), AuthError> {
    // Check operator exists
    let operator = registry
        .get_operator(&call.operator_id)
        .cloned()
        .ok_or_else(|| AuthError::UnknownOperator {
            operator_id: call.operator_id.clone(),
        })?;

    // Check permissions
    if !registry.has_role(&call.operator_id, required_role) {
        return Err(AuthError::InsufficientPermissions {
            operator_id: call.operator_id.clone(),
            required: required_role,
            actual: operator.role,
        });
    }

    // Check timestamp freshness
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if call.timestamp + MAX_OPERATION_AGE_SECS < now {
        return Err(AuthError::OperationExpired {
            timestamp: call.timestamp,
            current: now,
        });
    }

    // Check nonce
    registry.check_nonce(&call.operator_id, call.nonce, call.timestamp)?;

    // Verify signature
    let message = call.signing_message();
    let signature_bytes: [u8; 64] =
        call.signature
            .as_slice()
            .try_into()
            .map_err(|_| AuthError::MalformedSignature {
                reason: "invalid signature length or encoding".into(),
            })?;
    let signature = Signature::from_bytes(&signature_bytes);

    operator
        .verifying_key
        .verify(&message, &signature)
        .map_err(|_| AuthError::InvalidSignature {
            operator_id: call.operator_id.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    fn create_test_operator() -> (SigningKey, VerifyingKey, String) {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();
        let operator_id = "operator_001".to_string();
        (sk, vk, operator_id)
    }

    fn create_signed_call(
        sk: &SigningKey,
        operator_id: String,
        operation: Operation,
        nonce: u64,
    ) -> AuthorizedCall {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut call = AuthorizedCall {
            operation,
            timestamp,
            nonce,
            signature: vec![],
            operator_id,
        };

        let message = call.signing_message();
        let signature = sk.sign(&message);
        call.signature = signature.to_bytes().to_vec();
        call
    }

    #[test]
    fn authorized_operation_succeeds() {
        let (sk, vk, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        registry
            .register_operator(operator_id.clone(), vk, Role::Operator)
            .unwrap();

        let operation = Operation::ForceCircuitOpen;
        let call = create_signed_call(&sk, operator_id, operation, 1);

        assert!(verify_authorization(&call, &mut registry, Role::Operator).is_ok());
    }

    #[test]
    fn invalid_signature_rejected() {
        let (sk, vk, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        registry
            .register_operator(operator_id.clone(), vk, Role::Operator)
            .unwrap();

        let mut call = create_signed_call(&sk, operator_id, Operation::ForceCircuitOpen, 1);
        // Corrupt signature
        call.signature[0] ^= 0xFF;

        let result = verify_authorization(&call, &mut registry, Role::Operator);
        assert!(matches!(result, Err(AuthError::InvalidSignature { .. })));
    }

    #[test]
    fn insufficient_permissions_rejected() {
        let (sk, vk, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        // Register as Executor (lower role)
        registry
            .register_operator(operator_id.clone(), vk, Role::Executor)
            .unwrap();

        let operation = Operation::QueueDisbursement {
            proposal_id: "prop_001".into(),
            amount_ct: 10000,
        };
        let call = create_signed_call(&sk, operator_id, operation, 1);

        // Try to perform Operator-level action
        let result = verify_authorization(&call, &mut registry, Role::Operator);
        assert!(matches!(
            result,
            Err(AuthError::InsufficientPermissions { .. })
        ));
    }

    #[test]
    fn nonce_replay_rejected() {
        let (sk, vk, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        registry
            .register_operator(operator_id.clone(), vk, Role::Operator)
            .unwrap();

        let call = create_signed_call(&sk, operator_id, Operation::ForceCircuitOpen, 1);

        // First call succeeds
        assert!(verify_authorization(&call, &mut registry, Role::Operator).is_ok());

        // Second call with same nonce fails
        let result = verify_authorization(&call, &mut registry, Role::Operator);
        assert!(matches!(result, Err(AuthError::NonceReused { .. })));
    }

    #[test]
    fn unknown_operator_rejected() {
        let (sk, _, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        // Don't register operator

        let call = create_signed_call(&sk, operator_id, Operation::ForceCircuitOpen, 1);

        let result = verify_authorization(&call, &mut registry, Role::Operator);
        assert!(matches!(result, Err(AuthError::UnknownOperator { .. })));
    }

    #[test]
    fn role_hierarchy_works() {
        let (sk, vk, operator_id) = create_test_operator();
        let mut registry = OperatorRegistry::new();
        registry
            .register_operator(operator_id.clone(), vk, Role::Admin)
            .unwrap();

        // Admin can do Operator actions
        let call = create_signed_call(&sk, operator_id.clone(), Operation::ForceCircuitOpen, 1);
        assert!(verify_authorization(&call, &mut registry, Role::Operator).is_ok());

        // Admin can do Executor actions
        let call2 = create_signed_call(&sk, operator_id, Operation::ForceCircuitClosed, 2);
        assert!(verify_authorization(&call2, &mut registry, Role::Executor).is_ok());
    }
}
