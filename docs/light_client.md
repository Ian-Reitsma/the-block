# Light-Client Synchronization Guide

The `light-client` crate offers a minimal header verifier intended for mobile
and resource-constrained environments. It trades full validation for a compact
sync process that still detects blatant forks and stale peers.

## Sync Options

`SyncOptions` gate background synchronization based on device conditions:

```rust
pub struct SyncOptions {
    pub wifi_only: bool,
    pub require_charging: bool,
    pub min_battery: f32,
}
```

`sync_background(client, opts, fetch)` short-circuits when Wi‑Fi is unavailable,
the device is not charging, or the battery level falls below `min_battery`.
When the checks pass, it fetches headers starting from the client's tip via the
provided `fetch(start_height)` closure and verifies them before appending.
Real deployments should replace the stubbed `on_wifi`, `is_charging`, and
`battery_level` helpers with platform-specific checks in the mobile SDKs.

## Header Verification

Headers are represented by the simplified `Header` struct. `LightClient` keeps a
vector of headers and appends new ones after basic height checks:

```rust
pub fn verify_and_append(&mut self, h: Header) -> Result<(), ()> {
    // Verifies previous hash linkage, PoW difficulty and optional checkpoints
    // before appending.
}
```

A production client must additionally verify:

- BLAKE3 proof-of-work meets the advertised difficulty target.
- The previous hash matches the tip of the local chain.
- Validator signatures or finality proofs if operating in PoS mode.
- Checkpoint headers sourced from trusted channels.

### Checkpoint Invalidation

Trusted checkpoints can be revoked by governance in the event of a detected
fork. Clients must track checkpoint hashes by height and refuse headers whose
`checkpoint_hash` no longer matches the trusted list. When a checkpoint is
invalidated, the light client should roll back to the last valid height and
re-sync from that point to avoid following an obsolete chain.

### Header Cache Rules

Mobile deployments retain only a sliding window of recent headers to conserve
storage. Older entries beyond a few thousand blocks may be evicted, but any
height anchoring a trusted checkpoint must remain cached until the checkpoint
expires or is explicitly revoked. Cache eviction must never drop a header that
is still required to verify PoW linkage from the last checkpoint.

## Security Model

The demo implementation assumes a trusted bootstrap header.  Without cryptographic
proofs, an adversary could feed arbitrary headers.  Real deployments should ship
hard-coded checkpoints and verify cumulative work or stake before accepting
updates.

Because light clients only track headers, they rely on full nodes for transaction
proofs.  Any application using the light client must validate Merkle proofs for
account state or receipts before acting on them.

## Usage Example

```rust
use light_client::{Header, LightClient, SyncOptions, sync_background};

let genesis = Header { height: 0, ..Default::default() };
let mut lc = LightClient::new(genesis);
let opts = SyncOptions { wifi_only: true, require_charging: true, min_battery: 0.5 };
sync_background(&mut lc, opts, |_start| Vec::new());
```

## Further Reading

- [`crates/light-client/src/lib.rs`](../crates/light-client/src/lib.rs) — source
  code with stubs to be replaced by platform integrations.
- [`docs/mobile_light_client.md`](mobile_light_client.md) — mobile-specific notes
  and background sync strategies.
