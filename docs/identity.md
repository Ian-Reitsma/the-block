# Identity Registry
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Handles are normalized to lowercase NFC form and must not start with `sys/` or `admin/`.

## Signed message

Clients sign the BLAKE3 hash of the concatenation:

```
"register:" || handle_norm || pubkey || nonce_le
```

The resulting 32-byte digest is signed with Ed25519. The server verifies this
signature when binding a handle to the public key's hex address.

## Error codes

| Code           | Meaning                  |
|----------------|--------------------------|
| `E_DUP_HANDLE` | handle already registered |
| `E_BAD_SIG`    | signature verification failed |
| `E_LOW_NONCE`  | nonce not greater than previous |
| `E_RESERVED`   | handle uses a reserved prefix |

## Decentralized Identifier Anchors

Anchoring a DID document is performed with the `identity.anchor` RPC, which
accepts a serialized [`TxDidAnchor`](../node/src/transaction.rs) payload. The
payload contains the owning address, its Ed25519 public key, the canonical DID
document body, a strictly increasing nonce, and the owner's signature over

```
BLAKE3("did-anchor:" || address || hash(document) || nonce_le)
```

The registry stores the document and hash keyed by address and rejects any
update whose nonce is not greater than the previous value. Each successful
anchor increments the `did_anchor_total` Prometheus counter.

The JSON-RPC layer enforces the same monotonic requirement *per address* before
requests reach the registry. Nonces are scoped by the submitting address, so a
client can retry `nonce = 1` for two different addresses without tripping the
replay guard, while genuine replays for the same address are rejected
immediately.

An optional `remote_attestation` can accompany the request. When present, the
attestation must be signed by one of the governance-configured provenance
signers using the digest

```
BLAKE3("did-anchor-remote:" || address || hash(document) || nonce_le)
```

This allows operators to require remote signer approval while keeping the
anchored state independently verifiable.

### RPCs and Errors

- `identity.anchor` &mdash; anchors the supplied document. Errors surface as:
  - `E_DID_ADDR` invalid address/public key binding
  - `E_DID_KEY` malformed public key encoding
  - `E_DID_SIG` failed owner signature verification
  - `E_DID_REPLAY` nonce not strictly greater than the stored nonce
  - `E_DID_DOC_TOO_LARGE` document exceeds the 64&nbsp;KiB limit
  - `E_DID_ATTEST` malformed or invalid remote attestation
  - `E_DID_SIGNER` attestation signer not present in the provenance snapshot
  - `E_DID_REVOKED` governance has revoked the DID for this address
  - `E_DID_STORAGE` persistence failure

- `identity.resolve` &mdash; returns the latest anchored document, hash, nonce, and
  remote signer metadata for the provided address. Missing records surface with
  `null` fields.

Governance may revoke compromised identifiers via `GovStore::revoke_did`, which
blocks subsequent anchors until the revocation is cleared. Revocations and their
history are stored alongside other governance metadata for auditing.

### Explorer Integration

The explorer maintains a dedicated `did_records` table capturing the anchored
hash and timestamp for each address. Data is sourced from the embedded
`DidRegistry`, cached in-memory for repeated lookups, and surfaced through the
following endpoints:

- `GET /identity/dids/:address` &mdash; returns the latest anchored document,
  hash, nonce, and remote signer metadata. Results are cached so subsequent
  resolves avoid an extra RocksDB read.
- `GET /dids?limit=<n>` &mdash; lists the most recent anchors, including a
  `wallet_url` for one-click navigation to the owning account view.
- `GET /dids?address=<addr>` &mdash; emits the anchor history for the supplied
  address, newest first.
- `GET /dids/metrics/anchor_rate` &mdash; derives a per-second anchor rate from
  the archived `did_anchor_total` counter for dashboard plots.

Explorer responses are cached in-memory via an LRU keyed by address so repeated
resolves avoid extra RocksDB reads, and the anchor-rate endpoint derives its
series from the persisted `did_anchor_total` history. Governance revocations
block further anchors until cleared, and operators can correlate revocation
events with DID history by consulting the governance timeline alongside these
feeds.

The dedicated `did_view` page consumes these feeds to highlight recent DID
activity, link back to wallet detail panes, and graph anchor velocity.

### CLI Usage

The `contract light-client` CLI mirrors the RPC endpoints for local anchoring
and inspection workflows. A sample DID document lives at
[`examples/did.json`](../examples/did.json) and can be anchored with:

```
contract light-client did anchor examples/did.json \
    --nonce 1 \
    --secret <hex-encoded-owner-secret> \
    --rpc http://127.0.0.1:26658
```

When anchoring from operators that require provenance approval, pass
`--remote-signer <path>` where the file either contains a raw hex-encoded
Ed25519 secret key or a JSON object of the form:

```json
{
  "secret": "<remote-signer-secret-hex>",
  "signer": "<optional-signer-public-hex>"
}
```

The CLI derives and validates the signer hex against the provided secret before
producing the attestation. Supplying `--sign-only` skips the RPC submission and
prints the fully signed payload so that an offline system can broadcast it
later via `identity.anchor`.

Resolving a record uses the complementary subcommand:

```
contract light-client did resolve <address> --rpc http://127.0.0.1:26658
```

This emits a human-readable summary by default; add `--json` for structured
output suitable for scripts. Refresh shell completions after adding the new
subcommands with `contract completions <shell>`.

### Simulation

`sim/did.rs` drives a churn scenario by generating many DID anchors across a
configurable account set, pushing the resulting documents into the explorer and
recording metric snapshots. Inspect the resulting SQLite database (default
`did_sim/explorer.db`) or query the explorer API to analyse update rates and
history.