# Bridge Examples

Example payloads for cross-chain bridge operations, including header submissions and Merkle proofs.

## Files

### `header.json`

A sample external chain header used for light client verification. Relayers submit these headers to prove state on the source chain.

**Key Fields:**
- `chain_id` - Identifier for the external chain (e.g., `"ext"`)
- `height` - Block height on the source chain
- `merkle_root` - State root hash (32-byte hex)
- `signature` - Relayer signature attesting to header validity

**Usage:**
```bash
# Submit header via RPC (typically done by relayers)
tb-cli bridge submit-header --json header.json
```

### `proof.json`

A Merkle proof demonstrating inclusion of a transaction or state in a submitted header. Used when claiming bridged assets.

**Key Fields:**
- `leaf` - The data being proven (32-byte hex, typically a transaction hash)
- `path` - Array of sibling hashes forming the Merkle path to the root

**Usage:**
```bash
# Verify proof locally
tb-cli bridge verify-proof --header header.json --proof proof.json

# Claim bridged tokens with proof
tb-cli bridge claim --proof proof.json --destination ct1...
```

## How Bridge Verification Works

1. **Relayers** submit external chain headers (`header.json`)
2. **Light client** verifies headers form a valid chain
3. **Users** submit Merkle proofs (`proof.json`) to claim assets
4. **Bridge contract** verifies proof against submitted header's `merkle_root`

## Related Documentation

- [Bridge Architecture](../../docs/architecture.md#bridges-and-dex)
- [Bridge Source Code](../../bridges/src/lib.rs)
- [Light Client Implementation](../../bridges/src/light_client.rs)
