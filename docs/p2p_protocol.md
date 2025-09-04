# P2P Handshake and Capability Negotiation

Peers exchange a two-step handshake before participating in gossip. The
handshake verifies network identity, negotiates protocol versions, and records
supported feature bits for later routing and policy decisions.

## Message Flow

Every inbound connection begins with a serialized `Hello` structure:

```rust
struct Hello {
    network_id: [u8; 4],
    proto_version: u16,
    feature_bits: u32,
    agent: String,
    nonce: u64,
}
```

`network_id` identifies the chain; mismatches trigger a rejection. `proto_version`
represents the highest protocol version supported by the sender. `feature_bits`
is a bitmask of optional capabilities described below. The `agent` string is free
form and surfaced in metrics and RPC for debugging. `nonce` seeds connection-level
randomness and helps detect echo attacks.

The receiver responds with a `HelloAck`:

```rust
struct HelloAck {
    ok: bool,
    reason: Option<String>,
    features_accepted: u32,
    min_backoff_ms: u32,
}
```

`features_accepted` is the intersection of the sender's feature bits with the
receiver's `supported_features`. `min_backoff_ms` advises the sender how long to
wait before retrying after a rejection.

## Version Negotiation and Downgrades

The local node constructs a `HandshakeCfg` specifying the expected
`network_id`, a minimum protocol version, required feature bits, and the set of
features it understands:

```rust
pub struct HandshakeCfg {
    pub network_id: [u8; 4],
    pub min_proto: u16,
    pub required_features: u32,
    pub supported_features: u32,
}
```

`handle_handshake` enforces these rules:

1. `network_id` mismatch → `reason="bad_network"`
2. `proto_version < min_proto` → `reason="old_proto"`
3. Missing required feature bits → `reason="missing_features"`

Rejections increment
`p2p_handshake_reject_total{reason}`. Successful handshakes record
`p2p_handshake_accept_total{features}` using the hexadecimal
representation of `features_accepted` for the label value.

Nodes can downgrade gracefully by advertising a lower `proto_version`. As long as
it meets the peer's `min_proto`, the connection is accepted and only the
intersection of feature bits is enabled.  Operators bump their own
`min_proto` when deprecating older peers.

## Feature Bits

`FeatureBit` enumerates optional subsystems. Current assignments are:

```rust
pub enum FeatureBit {
    StorageV1      = 1 << 0,
    ComputeMarketV1= 1 << 1,
    GovV1          = 1 << 2,
    FeeRoutingV2   = 1 << 3,
}
```

Future features append additional bits.  When proposing a new capability, update
this enum and bump `proto_version` if the change is not backward compatible.

## Peer Registry

Accepted peers are stored in an in-memory registry keyed by connection ID. Each
entry retains the peer's reported `agent` string and the accepted feature mask.
This registry powers diagnostic RPCs and can be queried programmatically via
`p2p::handshake::list_peers()`:

```rust
let peers = list_peers();
for (id, info) in peers {
    println!("{} => {} features {:#x}", id, info.agent, info.features);
}
```

The registry is cleared on restart.  Future extensions may persist this state for
long-lived peer reputation tracking.

## Telemetry

The handshake module exports the following Prometheus metrics:

- `p2p_handshake_reject_total{reason}` – count of rejected handshakes grouped by
  `bad_network`, `old_proto`, or `missing_features`.
- `p2p_handshake_accept_total{features}` – number of successful handshakes, the
  label contains the hexadecimal accepted feature mask.
- `peer_rejected_total{reason="protocol"}` – peers dropped after a later
  version mismatch.

These counters aid in detecting network splits or gradual rollouts of new
features.

## Extending the Protocol

To add a new capability:

1. Define a new `FeatureBit` value.
2. Gate dependent code behind a feature check
   (`hello.feature_bits & FeatureBit::New as u32 != 0`).
3. Include the bit in `supported_features` for nodes that implement it.
4. Consider bumping `proto_version` if the change breaks compatibility.
5. Update tests and documentation to reference the new feature.

The fixture `node/tests/handshake_version.rs` demonstrates mismatched protocol
versions and feature negotiation. Use it as a template when introducing new
handshake logic.
