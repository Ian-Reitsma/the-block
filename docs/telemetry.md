# Telemetry Field Reference

This document describes the telemetry fields emitted by The Block node for observability and debugging.

## Common Fields

The following fields appear across various telemetry events:

- `subsystem` - The component or module emitting the event (e.g., "mempool", "consensus", "net")
- `op` - The operation being performed (e.g., "submit", "validate", "broadcast")
- `sender` - The transaction sender address or peer identifier
- `nonce` - Transaction sequence number for the sender account
- `reason` - Human-readable explanation for rejections, drops, or errors
- `code` - Numeric error code for programmatic handling
- `fpb` - Fee per byte in atomic units (BLOCK)

## Transaction Events

### Mempool Submission

When a transaction is submitted to the mempool:

| Field | Type | Description |
|-------|------|-------------|
| `subsystem` | string | Always "mempool" |
| `op` | string | Always "submit" |
| `sender` | string | Hex-encoded sender address |
| `nonce` | u64 | Transaction nonce |
| `fpb` | u64 | Fee per byte offered |

### Transaction Rejection

When a transaction is rejected:

| Field | Type | Description |
|-------|------|-------------|
| `subsystem` | string | Always "mempool" |
| `op` | string | Always "reject" |
| `sender` | string | Hex-encoded sender address |
| `nonce` | u64 | Transaction nonce |
| `reason` | string | Rejection reason (e.g., "nonce_gap", "insufficient_balance") |
| `code` | i32 | Numeric error code |

## Network Events

### Peer Drop

When a peer connection is dropped:

| Field | Type | Description |
|-------|------|-------------|
| `subsystem` | string | Always "net" |
| `op` | string | Always "peer_drop" |
| `reason` | string | Drop reason (e.g., "rate_limit", "timeout", "protocol_violation") |

## Fee Lane Events

### Fee Floor Updates

When dynamic fee pricing is recalculated:

| Field | Type | Description |
|-------|------|-------------|
| `subsystem` | string | Always "fee" |
| `op` | string | Always "floor_update" |
| `fpb` | u64 | New fee floor per byte |

## Governance Events

### Proposal Events

| Field | Type | Description |
|-------|------|-------------|
| `subsystem` | string | Always "governance" |
| `op` | string | Operation type |
| `reason` | string | Status or outcome reason |
| `code` | i32 | Result code |

## Best Practices

1. Use structured logging with consistent field names
2. Include `subsystem` and `op` in all events for filtering
3. Use `reason` for human-readable explanations
4. Use `code` for programmatic error handling
5. Emit `fpb` for any fee-related decisions

## See Also

- [Operations Guide - Telemetry Wiring](operations.md#telemetry-wiring)
- [Architecture - Telemetry and Instrumentation](architecture.md#telemetry-and-instrumentation)
