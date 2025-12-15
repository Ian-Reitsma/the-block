# Settlement Examples

Example payloads for settlement audit reports and receipt validation.

## Files

### `audit_report.json`

A sample settlement audit report showing epoch-level receipt validation results. Used by storage providers and validators to track settlement health.

**Key Fields:**
- `epoch` - The epoch number this report covers
- `receipts` - Total number of receipts processed
- `invalid` - Count of receipts that failed validation
- `invalid_receipts` - Array detailing each invalid receipt:
  - `id` - Receipt identifier (hex)
  - `reason` - Human-readable failure reason

**Common Failure Reasons:**
- `"bad signature"` - Receipt signature doesn't match provider's key
- `"expired"` - Receipt submitted after validity window
- `"duplicate"` - Receipt already claimed
- `"amount mismatch"` - Claimed amount doesn't match proof

**Usage:**
```bash
# Generate audit report for an epoch
contract-cli storage audit --epoch 42

# View settlement status
contract-cli storage settlement-status --epoch 42
```

## Settlement Flow

1. **Providers** submit service receipts during the epoch
2. **Validators** verify receipts against SNARK proofs
3. **Settlement** runs at epoch boundary, paying valid receipts
4. **Audit reports** summarize what was paid vs rejected

## Related Documentation

- [Storage Settlement](../../docs/architecture.md#storage-and-state)
- [Compute Settlement](../../docs/architecture.md#compute-marketplace)
- [CLI Storage Commands](../../docs/apis_and_tooling.md#cli-command-reference)
