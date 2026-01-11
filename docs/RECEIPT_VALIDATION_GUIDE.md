# Receipt Validation Quick Reference

**Date:** December 19, 2025
**Module:** `node/src/receipts_validation.rs`

---

## Constants

```rust
/// Maximum receipts allowed per block (DoS protection)
pub const MAX_RECEIPTS_PER_BLOCK: usize = 10_000;

/// Maximum total serialized receipt bytes per block (10MB)
pub const MAX_RECEIPT_BYTES_PER_BLOCK: usize = 10_000_000;

/// Maximum length for string fields (contract_id, provider, etc.)
pub const MAX_STRING_FIELD_LENGTH: usize = 256;

/// Minimum payment amount to emit a receipt (spam protection) in BLOCK
pub const MIN_PAYMENT_FOR_RECEIPT: u64 = 1;
```

---

## Validation Functions

### `validate_receipt(receipt: &Receipt, block_height: u64)`

Validates a single receipt's fields.

**Checks:**
- Block height matches expected height
- String fields non-empty and ≤ 256 chars
- Numeric fields > 0 for required fields

**Example:**
```rust
use crate::receipts_validation::validate_receipt;

let receipt = Receipt::Storage(StorageReceipt {
    contract_id: "sc_123".into(),
    provider: "provider_1".into(),
    bytes: 1000,
    price: 500,
    block_height: 100,
    provider_escrow: 10000,
});

// Validate before adding to block
if let Err(e) = validate_receipt(&receipt, 100) {
    eprintln!("Invalid receipt: {}", e);
}
```

---

### `validate_receipt_count(count: usize)`

Validates receipt count doesn't exceed maximum.

**Example:**
```rust
use crate::receipts_validation::validate_receipt_count;

let receipts = vec![/* ... */];

// Check before mining block
validate_receipt_count(receipts.len())?;
```

---

### `validate_receipt_size(bytes: usize)`

Validates total receipt size doesn't exceed maximum.

**Example:**
```rust
use crate::receipts_validation::validate_receipt_size;

let encoded = encode_receipts(&receipts)?;

// Check before mining block
validate_receipt_size(encoded.len())?;
```

---

## Error Types

```rust
pub enum ValidationError {
    TooManyReceipts { count: usize, max: usize },
    ReceiptsTooLarge { bytes: usize, max: usize },
    BlockHeightMismatch { receipt_height: u64, block_height: u64 },
    EmptyStringField { field: &'static str },
    StringFieldTooLong { field: &'static str, length: usize, max: usize },
    ZeroValue { field: &'static str },
}
```

All errors implement `Display` and `Error` traits.

---

## Telemetry Metrics

### New Metrics Added

1. **`receipt_encoding_failures_total`** (Counter)
   - Incremented when receipt encoding fails
   - Should always be 0 in production
   - Alert if > 0

2. **`receipt_validation_failures_total`** (Counter)
   - Incremented when receipt validation fails
   - Tracks malformed receipts from markets
   - Use for debugging market-side issues

**Usage:**
```rust
#[cfg(feature = "telemetry")]
use crate::telemetry::receipts::{
    RECEIPT_ENCODING_FAILURES_TOTAL,
    RECEIPT_VALIDATION_FAILURES_TOTAL,
};

// When encoding fails
#[cfg(feature = "telemetry")]
RECEIPT_ENCODING_FAILURES_TOTAL.inc();

// When validation fails
#[cfg(feature = "telemetry")]
RECEIPT_VALIDATION_FAILURES_TOTAL.inc();
```

---

## Integration Example

### Adding Validation to Block Construction

```rust
// Collect receipts from all markets
let mut block_receipts = Vec::new();
for receipt in drain_all_market_receipts() {
    block_receipts.push(receipt);
}

// Validate receipt count
if let Err(e) = validate_receipt_count(block_receipts.len()) {
    return Err(PyError::value(format!("Too many receipts: {}", e)));
}

// Validate individual receipts
for receipt in &block_receipts {
    if let Err(e) = validate_receipt(receipt, current_block_height) {
        #[cfg(feature = "telemetry")]
        RECEIPT_VALIDATION_FAILURES_TOTAL.inc();

        warn!(
            error = %e,
            receipt_type = receipt.market_name(),
            "Invalid receipt detected"
        );
        // Optionally filter out invalid receipts
    }
}

// Create block
let mut block = Block {
    receipts: block_receipts,
    // ... other fields
};

// Validate receipt size before mining
let encoded = encode_receipts(&block.receipts)?;
validate_receipt_size(encoded.len())?;

// Proceed with mining
```

---

## Best Practices

### 1. Always Validate Before Mining

```rust
// ✅ GOOD
validate_receipt_count(receipts.len())?;
let encoded = encode_receipts(&receipts)?;
validate_receipt_size(encoded.len())?;
start_mining(block);

// ❌ BAD
start_mining(block); // No validation!
```

### 2. Log Validation Failures

```rust
// ✅ GOOD
if let Err(e) = validate_receipt(&receipt, height) {
    #[cfg(feature = "telemetry")]
    RECEIPT_VALIDATION_FAILURES_TOTAL.inc();

    warn!(
        error = %e,
        receipt_type = receipt.market_name(),
        block_height = height,
        "Invalid receipt"
    );
}

// ❌ BAD
let _ = validate_receipt(&receipt, height); // Silent failure
```

### 3. Monitor Telemetry Metrics

```bash
# Check for encoding failures (should be 0)
curl localhost:9090/metrics | grep receipt_encoding_failures_total

# Check for validation failures
curl localhost:9090/metrics | grep receipt_validation_failures_total

# Check receipt counts per block
curl localhost:9090/metrics | grep receipts_per_block
```

### 4. Set Up Alerts

```yaml
# Prometheus alert rules
groups:
  - name: receipts
    rules:
      - alert: ReceiptEncodingFailure
        expr: receipt_encoding_failures_total > 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "CRITICAL: Receipt encoding is failing"

      - alert: ReceiptLimitApproaching
        expr: receipts_per_block > 8000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Receipt count approaching limit (10k)"

      - alert: ReceiptSizeApproaching
        expr: receipt_bytes_per_block > 8000000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Receipt size approaching limit (10MB)"
```

---

## Testing

### Unit Tests

```rust
#[test]
fn valid_receipt_passes() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "sc_123".into(),
        provider: "provider_1".into(),
        bytes: 1000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
    });

    assert!(validate_receipt(&receipt, 100).is_ok());
}

#[test]
fn empty_contract_id_fails() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "".into(), // Empty!
        provider: "provider_1".into(),
        bytes: 1000,
        price: 500,
        block_height: 100,
        provider_escrow: 10000,
    });

    assert!(matches!(
        validate_receipt(&receipt, 100),
        Err(ValidationError::EmptyStringField { field: "contract_id" })
    ));
}
```

### Integration Tests

```rust
#[test]
fn block_with_too_many_receipts_fails() {
    let mut blockchain = Blockchain::new();

    // Create 15,000 receipts (exceeds limit)
    for i in 0..15_000 {
        create_receipt(&mut blockchain, format!("receipt_{}", i));
    }

    // Mining should fail
    let result = blockchain.mine_block("miner");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Too many receipts"));
}
```

---

## Common Issues & Solutions

### Issue: "Too many receipts in block"

**Cause:** Receipt count exceeds `MAX_RECEIPTS_PER_BLOCK` (10,000)

**Solution:**
1. Check which market is emitting excessive receipts
2. Implement batching/aggregation in that market
3. Consider increasing settlement thresholds

### Issue: "Receipts too large"

**Cause:** Total serialized size exceeds `MAX_RECEIPT_BYTES_PER_BLOCK` (10 MB)

**Solution:**
1. Check for very long string fields (contract_id, provider)
2. Implement string length limits in markets
3. Consider receipt compression

### Issue: "Empty string field"

**Cause:** Market emitting receipts with empty contract_id or provider

**Solution:**
1. Add validation in market before emitting receipt
2. Check market-side receipt creation logic
3. Ensure all required fields populated

### Issue: "Block height mismatch"

**Cause:** Receipt block_height doesn't match current block

**Solution:**
1. Ensure market receives correct block height
2. Check `set_current_block()` called before drain
3. Verify no race conditions in block height assignment

---

## Performance Characteristics

### Validation Overhead

| Operation | Time | Notes |
|-----------|------|-------|
| `validate_receipt_count()` | ~1 ns | Simple comparison |
| `validate_receipt()` (per receipt) | ~50-100 ns | String length checks |
| `validate_receipt_size()` | ~1 ns | Simple comparison |
| **Total per block (1000 receipts)** | **~100 μs** | Negligible overhead |

### Memory Usage

| Receipts | Raw Size | Serialized Size | With Overhead |
|----------|----------|-----------------|---------------|
| 100 | ~11 KB | ~15 KB | ~50 KB |
| 1,000 | ~110 KB | ~150 KB | ~500 KB |
| 10,000 (max) | ~1.1 MB | ~1.5 MB | ~5 MB |

---

## See Also

- [docs/archive/CRITICAL_FIXES_COMPLETE.md](docs/archive/CRITICAL_FIXES_COMPLETE.md) - Complete implementation details
- [docs/instructions.md](docs/instructions.md) - Comprehensive audit report
- [node/src/receipts_validation.rs](node/src/receipts_validation.rs) - Full implementation
- [node/tests/receipt_integration.rs](node/tests/receipt_integration.rs) - Integration tests

---

**Last Updated:** December 19, 2025
