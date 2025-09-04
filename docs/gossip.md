# Gossip Relay Semantics

The gossip layer relays messages between peers while suppressing duplicates and
constraining bandwidth.  `node/src/gossip/relay.rs` implements a TTL-based hash
of serialized messages:

```rust
pub fn should_process(&self, msg: &Message) -> bool {
    let h = hash(&bincode::serialize(msg).unwrap_or_default());
    let mut guard = self.recent.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    guard.retain(|_, t| now.duration_since(*t) < self.ttl);
    if guard.contains_key(&h) {
        GOSSIP_DUPLICATE_TOTAL.inc();
        false
    } else {
        guard.insert(h, now);
        true
    }
}
```

The default TTL is two seconds.  Any message seen within that window is dropped
and `gossip_duplicate_total` increments.  Telemetry consumers can monitor this
counter to diagnose misbehaving peers or replay storms.

## Fanout Selection

When a message passes the duplicate check, the relay chooses a random subset of
peers to forward it to.  The fanout size is `ceil(sqrt(N))` capped at 16, where
`N` is the current number of connected peers.  This produces logarithmic spread
without broadcasting to everyone at once.  The chosen fanout is exposed via the
`gossip_fanout_gauge` metric.

Setting the environment variable `TB_GOSSIP_FANOUT=all` disables the random
selection and forces broadcast to every peer.  This override is useful for
small testnets where full fanout is desired.

The selection procedure shuffles the peer list with `rand::thread_rng` and sends
to the first `fanout` entries:

```rust
let mut list = peers.to_vec();
if !fanout_all {
    list.shuffle(&mut rng);
}
for addr in list.into_iter().take(fanout) {
    send(addr, msg);
}
```

Integration tests in `node/tests/gossip_relay.rs` assert that duplicate messages
are dropped and that the computed fanout stays within the expected range even
under packet loss.  The `node/tests/turbine.rs` harness verifies that the
deterministic Turbine tree reaches all peers when the relay fanout equals the
computed `sqrt(N)`.

## Operational Guidance

- Monitor `gossip_duplicate_total` for spikes indicating loops or floods.
- Track `gossip_fanout_gauge` to ensure the relay adapts as peers join or leave.
- Use `TB_GOSSIP_FANOUT=all` only in controlled environments; it negates the
  bandwidth savings of adaptive fanout.
- The default TTL of two seconds balances duplicate suppression with tolerance
  for legitimate replays.  Adjust `Relay::new(ttl)` if your deployment requires
  a different window.

See [`docs/networking.md`](networking.md) for peer database recovery and
[`docs/gossip_chaos.md`](gossip_chaos.md) for adversarial gossip testing.
