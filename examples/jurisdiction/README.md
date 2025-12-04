# Jurisdiction Examples

Example jurisdiction policy configurations for different regulatory regions.

## Files

### `us.json`

Sample US jurisdiction policy pack.

```json
{
  "region": "US",
  "consent_required": true,
  "features": ["wallet", "dex"]
}
```

**Key Fields:**
- `region` - ISO region code
- `consent_required` - Whether explicit user consent is required for data operations
- `features` - List of enabled features for this jurisdiction

### `eu.json`

Sample EU jurisdiction policy pack (GDPR-aligned).

```json
{
  "region": "EU",
  "consent_required": false,
  "features": ["staking"]
}
```

Note: `consent_required: false` here means the system handles consent via GDPR-compliant defaults rather than per-operation prompts.

## Using Jurisdiction Packs

```bash
# List available jurisdiction packs
tb-cli jurisdiction list

# Set jurisdiction for current node/account
tb-cli jurisdiction set --pack US

# Check current jurisdiction status
tb-cli jurisdiction status

# Reset to governance defaults
tb-cli jurisdiction reset
```

## How Jurisdictions Affect Behavior

| Setting | Effect |
|---------|--------|
| `consent_required` | Enables/disables per-operation consent prompts |
| `features` | Restricts which protocol features are available |
| Fee modifiers | Per-jurisdiction surcharges (defined in full pack) |
| LE logging | Law-enforcement audit logging requirements |

## Full Policy Packs

These examples show simplified configs. Full policy packs in `crates/jurisdiction/policies/*.toml` include:
- Read/write quotas
- Fee modifiers
- Feature toggles
- Audit logging flags
- Version and timeframe metadata

## Related Documentation

- [Jurisdiction Packs Spec](../../docs/world-os-spec/05-jurisdiction-packs.md)
- [KYC and Compliance](../../docs/security_and_privacy.md#kyc-jurisdiction-and-compliance)
- [RPC Jurisdiction Methods](../../docs/apis_and_tooling.md#json-rpc)
