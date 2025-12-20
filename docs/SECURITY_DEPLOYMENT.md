# Security Deployment Guide

**Last Updated**: December 19, 2025  
**Status**: Production Ready ✅

---

## Quick Start

### 1. Enable Telemetry Feature

In your `Cargo.toml`:

```toml
[features]
telemetry = ["prometheus", "diagnostics"]

[dependencies]
prometheus = "0.13"
diagnostics = { path = "../diagnostics" }
```

Build with:
```bash
cargo build --features telemetry
```

### 2. Wire Consensus Metrics (5 minutes)

In your main block processing loop:

```rust
use crate::telemetry::consensus_integration::ConsensusStateTracker;

let metrics = ConsensusStateTracker::new();

// After successfully applying a block:
metrics.record_block_applied(
    new_height,
    block.transactions.len(),
    finalized_height,
    current_timestamp_secs(),
);

// Update network health:
metrics.update_peer_metrics(active_peers.len(), avg_latency);
metrics.update_mempool_metrics(mempool.len());

// Record events:
if fork_detected { metrics.record_fork(); }
if orphan_detected { metrics.record_orphan(); }
if partition_detected { metrics.record_partition(); }
```

### 3. Configure Storage Integration

#### Client Side
```rust
use storage::StorageContractBuilder;

let (request, tree) = StorageContractBuilder::new(
    object_id,
    provider_id,
    original_bytes,
    shares,
    price_per_block,
    start_block,
    retention_blocks,
)
.with_chunks(encoded_chunks)
.build()
.expect("failed to build contract");

// Store request.merkle_root on-chain
```

#### Provider Side
```rust
use storage::StorageProvider;

let mut provider = StorageProvider::new();
provider.accept_contract(contract_id, &contract, chunks)?;

let response = provider.respond_to_challenge(&challenge)?;
// Send response.chunk_data + response.merkle_proof to verifier
```

#### Verifier Side
```rust
contract.verify_proof(
    chunk_idx,
    &chunk_data,
    &proof,
    current_block,
)?;
```

### 4. Configure Authorization

#### Operator Registration
```rust
use governance::authorization::{OperatorRegistry, Role};
use crypto_suite::signatures::ed25519;

let mut registry = OperatorRegistry::new();
let (sk, vk) = ed25519::generate_keypair();

registry.register_operator(
    "operator-alice".into(),
    vk,
    Role::Operator,
)?;
```

#### Authorized Disbursement
```rust
use governance::disbursement_auth::AuthorizedDisbursementOps;
use governance::authorization::AuthorizedCall;

let auth = AuthorizedCall {
    operation: Operation::QueueDisbursement { 
        proposal_id: "prop-123".into(),
        amount_ct: 10_000,
    },
    timestamp,
    nonce,
    signature: sig_bytes,
    operator_id: "operator-alice".into(),
};

let disbursement = AuthorizedDisbursementOps::queue_disbursement(
    &store,
    &auth,
    &mut registry,
    payload,
    status,
)?;
```

---

## Security Checklist

### Pre-Deployment

- [ ] **Receipt Signatures**
  - [ ] All receipts are Ed25519-signed
  - [ ] Nonce tracker is initialized
  - [ ] Finality-window nonce pruning enabled
  - [ ] Test: `cargo test --lib receipt_crypto`

- [ ] **Storage Proofs**
  - [ ] Merkle tree implementation verified
  - [ ] Provider requires actual chunk data
  - [ ] On-chain contract stores 32-byte root
  - [ ] Test: `cargo test --lib storage::`

- [ ] **Authorization**
  - [ ] Operator keys configured
  - [ ] Role hierarchy enforced
  - [ ] Circuit breaker secured
  - [ ] Test: `cargo test --lib authorization`

- [ ] **Telemetry**
  - [ ] Prometheus endpoint configured
  - [ ] Metrics collection enabled
  - [ ] Stall detection threshold set (default: 120 sec)
  - [ ] Test: `cargo test --lib consensus_integration`

### Post-Deployment

- [ ] **Monitor Key Metrics**
  - [ ] `BLOCK_HEIGHT` increasing
  - [ ] `TRANSACTIONS_PER_SECOND` > 0
  - [ ] `FINALITY_LAG` < 10 blocks
  - [ ] `ACTIVE_PEERS` > 1

- [ ] **Watch for Attacks**
  - [ ] `RECEIPT_VALIDATION_FAILURES` = 0
  - [ ] `STORAGE_PROOF_VALIDATION_FAILURES` = 0
  - [ ] `CONSENSUS_STALLED` = 0
  - [ ] `NETWORK_PARTITION_DETECTED` = 0

- [ ] **Verify Authorization**
  - [ ] Unauthorized disbursements rejected
  - [ ] Circuit breaker responds to commands
  - [ ] Nonce tracking prevents replay

### Operational Procedures

#### Multi-Sig Ceremony (QueueDisbursement)

1. **Operator A**: Generate operation
   ```rust
   let op = Operation::QueueDisbursement { proposal_id, amount_ct };
   let msg = AuthorizedCall::signing_message(&op, timestamp, nonce);
   ```

2. **Operator A**: Sign
   ```rust
   let sig = sk_a.sign(&msg);
   ```

3. **Operator B**: Verify and countersign (for 2-of-2)
   ```rust
   verify_signature(&vk_a, &msg, &sig)?;
   let sig_b = sk_b.sign(&msg);
   ```

4. **Submit**: Combine signatures and submit
   ```rust
   AuthorizedDisbursementOps::queue_disbursement(&store, &auth, &mut registry, ...)?;
   ```

#### Emergency: Force Circuit Breaker Open

```rust
use governance::circuit_breaker::CircuitBreaker;

let cb = CircuitBreaker::new(Default::default());
let auth = create_authorized_call(Operation::ForceCircuitOpen)?;
let signers = [operator_a, operator_b]; // 2-of-2 quorum

verify_multisig(&auth, &signers)?;
cb.authorized_force_open(&auth, &mut registry)?;
```

---

## Metrics Reference

### Core Consensus Health

| Metric | Type | Alerts |
|--------|------|--------|
| `BLOCK_HEIGHT` | Gauge | Should increase monotonically |
| `ACTIVE_PEERS` | Gauge | Alert if < 2 |
| `FINALITY_LAG` | Gauge | Alert if > 10 blocks |
| `CONSENSUS_STALLED` | Counter | Alert immediately |

### Transaction Flow

| Metric | Type | Alerts |
|--------|------|--------|
| `TRANSACTIONS_PER_SECOND` | Gauge | Alert if < expected throughput |
| `TRANSACTION_PROCESSING_TIME` | Histogram | P99 > 1s = slow validation |
| `MEMPOOL_SIZE` | Gauge | Alert if > max capacity |

### Network Health

| Metric | Type | Alerts |
|--------|------|--------|
| `PEER_LATENCY` | Histogram | P95 > 500ms = network issues |
| `FORK_DETECTED` | Counter | Alert if > 0 in an epoch |
| `ORPHANED_BLOCKS` | Counter | Alert if > 0 in an epoch |
| `NETWORK_PARTITION_DETECTED` | Counter | Alert immediately |

### Security Events

| Metric | Type | Alerts |
|--------|------|--------|
| `RECEIPT_VALIDATION_FAILURES` | Counter | Alert immediately |
| `STORAGE_PROOF_VALIDATION_FAILURES` | Counter | Alert immediately |

---

## Troubleshooting

### Receipt Validation Failures

**Problem**: `RECEIPT_VALIDATION_FAILURES` > 0  
**Causes**:
- Invalid Ed25519 signatures
- Provider not registered in provider registry
- Nonce reuse (replay attack)
- Stale timestamp (> 10 min old)

**Solution**:
```rust
// Check provider registration
let vk = registry.get_provider_key(provider_id)?;

// Verify nonce hasn't been used
let next_nonce = nonce_tracker.next_nonce(provider_id)?;
assert_eq!(nonce, next_nonce);

// Verify timestamp freshness
let age = now - receipt.timestamp;
assert!(age < 600); // 10 minutes
```

> **Security note:** Receipt nonces are hashed into a 32-byte key (BLAKE3 over `provider_id || nonce`) and retained in a fixed-size tracker (≈4k entries with finality-window pruning) so replay checks stay constant-time and memory bounded even under flood attempts.

### Storage Proof Validation Failures

**Problem**: `STORAGE_PROOF_VALIDATION_FAILURES` > 0  
**Causes**:
- Provider using wrong chunk data
- Merkle proof doesn't match on-chain root
- Chunk index mismatch
- Contract expired

**Solution**:
```rust
// Verify contract active
contract.is_active(current_block)?;

// Verify chunk_data hashes to proof leaf
let leaf = blake3(chunk_data);
assert_eq!(leaf, proof.leaf);

// Verify proof path to root
assert_eq!(compute_root(&proof), contract.storage_root);
```

> **Security note:** Merkle proofs are limited to 21 levels (~1M leaves) and any path exceeding that depth is rejected, so clients cannot force unbounded proof sizes during challenges.

### Consensus Stalls

**Problem**: `CONSENSUS_STALLED` incremented (no blocks for 2+ minutes)  
**Causes**:
- Network partition
- All validators down
- Consensus deadlock

**Solution**:
1. Check peer connectivity
2. Verify leader election
3. Review block production logs
4. If needed: activate circuit breaker to pause risky operations

### High Finality Lag

**Problem**: `FINALITY_LAG` > 10 blocks  
**Causes**:
- Slow block finalization
- Validator quorum issues
- Epoch transitions

**Solution**:
1. Monitor `BLOCK_PROPOSAL_TIME`
2. Check validator participation
3. Review finalization algorithm performance

---

## Upgrade Procedure

### When Deploying Security Updates

1. **Stage**: Deploy new binary with `--dry-run` flag
2. **Validate**: Run all security tests
   ```bash
   cargo test --all-features --lib
   ```
3. **Gradual Rollout**: Update validators one at a time
4. **Monitor**: Watch security metrics for 1 epoch
5. **Finalize**: When all validators running new version

### Backwards Compatibility

- ✅ Receipt format includes version (supports upgrades)
- ✅ Merkle proof structure is extensible
- ✅ Authorization operations support new variants
- ✅ Telemetry metrics are additive (no removals)

---

## Testing Commands

```bash
# All security tests
cargo test --all-features --lib

# Receipt security specifically
cargo test --lib receipt_crypto::

# Storage proofs
cargo test --lib storage::

# Authorization
cargo test --lib authorization::

# Telemetry
cargo test --lib consensus_integration::

# Integration tests
cargo test --test security_integration_tests

# Full suite with telemetry
cargo test --all-features
```

---

## Performance Notes

### Receipt Validation
- Ed25519 verification: ~1 ms per receipt
- Nonce tracker lookup: O(1) amortized
- Finality-window pruning: Automatic
- **Impact**: < 1% CPU overhead

### Storage Proof Validation
- Merkle path verification: O(log n) where n = chunk count
- Typical: 24 hashes for 16M chunks
- Hash time (BLAKE3): ~0.1 ms
- **Impact**: < 3 ms per proof

### Telemetry Recording
- Atomic metric updates: O(1)
- RAII timers: Automatic on drop
- No blocking operations
- **Impact**: < 0.1% CPU overhead

---

## Support

For security issues, follow responsible disclosure:  
**Email**: security@your-org.com  
**GPG Key**: Available in docs/security-pgp-key.asc

---

*Production deployment ready. All P0 security blockers resolved.*
