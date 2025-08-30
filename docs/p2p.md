# P2P Handshake

Nodes negotiate protocol compatibility before gossip. Each connection begins with a `Hello` message:

```
struct Hello {
    network_id: [u8;4],
    proto_version: u16,
    feature_bits: u32,
    agent: String,
    nonce: u64,
}
```

Peers reply with `HelloAck`:

```
struct HelloAck {
    ok: bool,
    reason: Option<String>,
    features_accepted: u32,
    min_backoff_ms: u32,
}
```

Handshake rejections increment `p2p_handshake_reject_total{reason}`, while successful exchanges record `p2p_handshake_accept_total{features}`.
The `proto_version` field gates compatibility. A node disconnects peers that advertise a different protocol version and increments `peer_rejected_total{reason="protocol"}`. The `agent` string and accepted features are retained per peer and surfaced via RPC for observability.

Example rejection:

```
Hello { proto_version: 2 } -> peer running version 1
disconnect (peer_rejected_total{reason="protocol"}++)
```

See [`node/tests/handshake_version.rs`](../node/tests/handshake_version.rs) to reproduce a mismatched handshake.
