# Governance Examples

Example payloads for governance operations, particularly treasury disbursements.

## Files

### `disbursement_example.json`

A complete example of a treasury disbursement proposal payload. Use this as a template when creating disbursement proposals via:

```bash
tb-cli gov disburse preview --json disbursement_example.json
tb-cli gov disburse create --json disbursement_example.json
```

**Key Fields:**
- `id` - Unique identifier for the disbursement
- `title` - Human-readable proposal title
- `amount_ct` - Total CT to disburse
- `amount_it` - Industrial share (sub-ledger accounting, not a separate token)
- `destination` - Recipient address (must start with `ct1`)
- `expected_receipts` - Breakdown of where funds go (must sum to `amount_ct`/`amount_it`)
- `timelock_epochs` - Waiting period after approval before execution

**Validation Rules:**
- `expected_receipts` totals must match `amount_ct` and `amount_it`
- All addresses must be valid `ct1...` format
- See `governance/src/treasury.rs::validate_disbursement_payload()` for full validation

## Related Documentation

- [Treasury Disbursements](../../docs/economics_and_governance.md#treasury-and-disbursements)
- [Governance CLI](../../docs/apis_and_tooling.md#cli-command-reference)
- [Disbursement Schema](../../docs/spec/disbursement.schema.json)
