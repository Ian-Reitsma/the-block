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

`sync_background(opts)` short-circuits when Wi‑Fi is unavailable, the device is
not charging, or the battery level falls below `min_battery`. Real deployments
should replace the stubbed `on_wifi`, `is_charging`, and `battery_level` helpers
with platform-specific checks in the mobile SDKs.

## Header Verification

Headers are represented by the simplified `Header` struct. `LightClient` keeps a
vector of headers and appends new ones after basic height checks:

```rust
pub fn verify_and_append(&mut self, h: Header) -> Result<(), ()> {
    // Real implementation should verify PoW difficulty, signatures, and
    // parent linkage.  The demo simply appends for now.
    self.chain.push(h);
    Ok(())
}
```

A production client must additionally verify:

- BLAKE3 proof-of-work meets the advertised difficulty target.
- The previous hash matches the tip of the local chain.
- Validator signatures or finality proofs if operating in PoS mode.
- Checkpoint headers sourced from trusted channels.

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
use light_client::{Header, LightClient, SyncOptions};

let mut lc = LightClient::new(Header { height: 0 });
let opts = SyncOptions { wifi_only: true, require_charging: true, min_battery: 0.5 };
light_client::sync_background(opts);
lc.verify_and_append(Header { height: 1 }).unwrap();
```

## Further Reading

- [`crates/light-client/src/lib.rs`](../crates/light-client/src/lib.rs) — source
  code with stubs to be replaced by platform integrations.
- [`docs/mobile_light_client.md`](mobile_light_client.md) — mobile-specific notes
  and background sync strategies.
