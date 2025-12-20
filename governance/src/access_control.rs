//! Governance Access Control - Signature-Based Authorization
//!
//! Prevents unauthorized access to critical treasury and circuit breaker operations.
//! All sensitive operations require Ed25519 signatures from registered operators.

use crypto_suite::hashing::blake3;
use crypto_suite::signatures::ed25519::{Signature, SigningKey, VerifyingKey};
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};

/// Operator roles with different privilege levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Role {
    /// Can execute queued disbursements (lowest privilege)
    Executor,
    /// Can queue disbursements and control circuit breaker
    Operator,
    /// Can modify roles and upgrade system (highest privilege)
    Governance,
}

/// Registry of authorized operators and their roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct OperatorRegistry {
    /// Map of operator public key -> (roles, added_at_epoch)
    operators: HashMap<VerifyingKey, (HashSet<Role>, u64)>,
}

impl OperatorRegistry {
    pub fn new() -> Self {
        Self {
            operators: HashMap::new(),
        }
    }

    /// Register an operator with specific roles
    pub fn register_operator(
        &mut self,
        verifying_key: VerifyingKey,
        roles: HashSet<Role>,
        epoch: u64,
    ) -> Result<(), String> {
        if roles.is_empty() {
            return Err("cannot register operator with no roles".into());
        }
        self.operators.insert(verifying_key, (roles, epoch));
        Ok(())
    }

    /// Check if operator has required role
    pub fn has_role(&self, verifying_key: &VerifyingKey, role: Role) -> bool {
        self.operators
            .get(verifying_key)
            .map(|(roles, _)| roles.contains(&role))
            .unwrap_or(false)
    }

    /// Get all roles for an operator
    pub fn get_roles(&self, verifying_key: &VerifyingKey) -> Option<&HashSet<Role>> {
        self.operators.get(verifying_key).map(|(roles, _)| roles)
    }

    /// Remove an operator (governance only)
    pub fn revoke_operator(&mut self, verifying_key: &VerifyingKey) {
        self.operators.remove(verifying_key);
    }

    pub fn operator_count(&self) -> usize {
        self.operators.len()
    }
}

/// Authorized operation with cryptographic proof
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AuthorizedCall {
    /// Operation identifier (e.g., "queue_disbursement", "force_open_circuit")
    pub operation: String,
    /// Timestamp (seconds since epoch) for freshness check
    pub timestamp: u64,
    /// Unique nonce to prevent replay attacks
    pub nonce: u64,
    /// Ed25519 signature over (operation || timestamp || nonce)
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub signature: Vec<u8>,
    /// Public key of the signing operator
    pub signer: VerifyingKey,
}

impl AuthorizedCall {
    /// Create a new authorized call (for testing/operator tools)
    pub fn new(operation: String, timestamp: u64, nonce: u64, signing_key: &SigningKey) -> Self {
        let signer = signing_key.verifying_key();

        // Build deterministic preimage
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"governance_auth");
        hasher.update(operation.as_bytes());
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(&nonce.to_le_bytes());
        let msg = hasher.finalize();

        let sig = signing_key.sign(msg.as_bytes());
        let signature = sig.to_bytes().to_vec();

        Self {
            operation,
            timestamp,
            nonce,
            signature,
            signer,
        }
    }

    /// Verify the signature
    fn verify_signature(&self) -> Result<(), String> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"governance_auth");
        hasher.update(self.operation.as_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.nonce.to_le_bytes());
        let msg = hasher.finalize();

        let signature_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| "malformed signature".to_string())?;
        let signature = Signature::from_bytes(&signature_bytes);

        self.signer
            .verify(msg.as_bytes(), &signature)
            .map_err(|e| format!("signature verification failed: {}", e))
    }
}

/// Nonce tracker for replay attack prevention
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct AuthNonceTracker {
    /// Map of (operator_key, nonce) -> timestamp when seen
    seen: HashMap<(VerifyingKey, u64), u64>,
    /// Grace period for pruning old nonces (seconds)
    grace_period: u64,
}

impl AuthNonceTracker {
    pub fn new(grace_period: u64) -> Self {
        Self {
            seen: HashMap::new(),
            grace_period,
        }
    }

    /// Check nonce is valid and record it
    pub fn check_and_record(
        &mut self,
        operator: VerifyingKey,
        nonce: u64,
        timestamp: u64,
    ) -> Result<(), String> {
        let key = (operator, nonce);
        if self.seen.contains_key(&key) {
            return Err(format!("nonce {} already used", nonce));
        }
        self.seen.insert(key, timestamp);
        Ok(())
    }

    /// Prune nonces older than grace period
    pub fn prune_old(&mut self, current_timestamp: u64) {
        let cutoff = current_timestamp.saturating_sub(self.grace_period);
        self.seen.retain(|_, &mut ts| ts >= cutoff);
    }
}

/// Authorization errors
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum AuthError {
    InvalidSignature { reason: String },
    UnauthorizedOperator { operator: String },
    InsufficientPrivilege { required: String, has: String },
    NonceReused { nonce: u64 },
    RequestExpired { timestamp: u64, current: u64 },
    MalformedRequest { reason: String },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature { reason } => write!(f, "Invalid signature: {}", reason),
            Self::UnauthorizedOperator { operator } => {
                write!(f, "Unauthorized operator: {}", operator)
            }
            Self::InsufficientPrivilege { required, has } => {
                write!(
                    f,
                    "Insufficient privilege: requires {} but has {}",
                    required, has
                )
            }
            Self::NonceReused { nonce } => write!(f, "Nonce {} already used", nonce),
            Self::RequestExpired { timestamp, current } => {
                write!(f, "Request expired: {} vs current {}", timestamp, current)
            }
            Self::MalformedRequest { reason } => write!(f, "Malformed request: {}", reason),
        }
    }
}

impl std::error::Error for AuthError {}

/// Authorization context for governance operations
pub struct AuthContext {
    pub operator_registry: OperatorRegistry,
    pub nonce_tracker: AuthNonceTracker,
    pub request_expiry_seconds: u64,
}

impl AuthContext {
    pub fn new(request_expiry_seconds: u64) -> Self {
        Self {
            operator_registry: OperatorRegistry::new(),
            nonce_tracker: AuthNonceTracker::new(request_expiry_seconds * 2),
            request_expiry_seconds,
        }
    }

    /// Authorize an operation requiring specific role
    pub fn authorize(
        &mut self,
        auth: &AuthorizedCall,
        required_role: Role,
    ) -> Result<(), AuthError> {
        // Verify signature
        auth.verify_signature()
            .map_err(|reason| AuthError::InvalidSignature { reason })?;

        // Check operator is registered
        if !self.operator_registry.operators.contains_key(&auth.signer) {
            return Err(AuthError::UnauthorizedOperator {
                operator: format!("{:?}", auth.signer),
            });
        }

        // Check operator has required role
        if !self.operator_registry.has_role(&auth.signer, required_role) {
            let roles = self
                .operator_registry
                .get_roles(&auth.signer)
                .map(|r| format!("{:?}", r))
                .unwrap_or_else(|| "none".to_string());
            return Err(AuthError::InsufficientPrivilege {
                required: format!("{:?}", required_role),
                has: roles,
            });
        }

        // Check timestamp freshness
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();

        if auth.timestamp < now.saturating_sub(self.request_expiry_seconds) {
            return Err(AuthError::RequestExpired {
                timestamp: auth.timestamp,
                current: now,
            });
        }

        // Check nonce
        self.nonce_tracker
            .check_and_record(auth.signer.clone(), auth.nonce, auth.timestamp)
            .map_err(|_| AuthError::NonceReused { nonce: auth.nonce })?;

        Ok(())
    }

    /// Prune old nonces
    pub fn prune(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();
        self.nonce_tracker.prune_old(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    fn create_test_keypair() -> (SigningKey, VerifyingKey) {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = SigningKey::generate(&mut rng);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
    }

    #[test]
    fn authorized_operator_succeeds() {
        let (sk, vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        let mut roles = HashSet::new();
        roles.insert(Role::Operator);
        ctx.operator_registry
            .register_operator(vk, roles, 0)
            .expect("register");

        let auth = AuthorizedCall::new("queue_disbursement".into(), current_timestamp(), 1, &sk);

        let result = ctx.authorize(&auth, Role::Operator);
        assert!(result.is_ok());
    }

    #[test]
    fn unauthorized_operator_rejected() {
        let (sk, _vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        // Operator not registered

        let auth = AuthorizedCall::new("queue_disbursement".into(), current_timestamp(), 1, &sk);

        let result = ctx.authorize(&auth, Role::Operator);
        assert!(matches!(
            result,
            Err(AuthError::UnauthorizedOperator { .. })
        ));
    }

    #[test]
    fn insufficient_privilege_rejected() {
        let (sk, vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        let mut roles = HashSet::new();
        roles.insert(Role::Executor); // Lower privilege
        ctx.operator_registry
            .register_operator(vk, roles, 0)
            .expect("register");

        let auth = AuthorizedCall::new("queue_disbursement".into(), current_timestamp(), 1, &sk);

        // Requires Operator role, but only has Executor
        let result = ctx.authorize(&auth, Role::Operator);
        assert!(matches!(
            result,
            Err(AuthError::InsufficientPrivilege { .. })
        ));
    }

    #[test]
    fn replay_attack_prevented() {
        let (sk, vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        let mut roles = HashSet::new();
        roles.insert(Role::Operator);
        ctx.operator_registry
            .register_operator(vk, roles, 0)
            .expect("register");

        let auth = AuthorizedCall::new("queue_disbursement".into(), current_timestamp(), 1, &sk);

        // First call succeeds
        assert!(ctx.authorize(&auth, Role::Operator).is_ok());

        // Second call with same nonce fails
        let result = ctx.authorize(&auth, Role::Operator);
        assert!(matches!(result, Err(AuthError::NonceReused { .. })));
    }

    #[test]
    fn expired_request_rejected() {
        let (sk, vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        let mut roles = HashSet::new();
        roles.insert(Role::Operator);
        ctx.operator_registry
            .register_operator(vk, roles, 0)
            .expect("register");

        // Create auth with old timestamp
        let old_timestamp = current_timestamp() - 120; // 2 minutes ago
        let auth = AuthorizedCall::new("queue_disbursement".into(), old_timestamp, 1, &sk);

        let result = ctx.authorize(&auth, Role::Operator);
        assert!(matches!(result, Err(AuthError::RequestExpired { .. })));
    }

    #[test]
    fn invalid_signature_rejected() {
        let (sk, vk) = create_test_keypair();

        let mut ctx = AuthContext::new(60);
        let mut roles = HashSet::new();
        roles.insert(Role::Operator);
        ctx.operator_registry
            .register_operator(vk, roles, 0)
            .expect("register");

        let mut auth =
            AuthorizedCall::new("queue_disbursement".into(), current_timestamp(), 1, &sk);

        // Corrupt signature
        auth.signature[0] ^= 0xFF;

        let result = ctx.authorize(&auth, Role::Operator);
        assert!(matches!(result, Err(AuthError::InvalidSignature { .. })));
    }
}
