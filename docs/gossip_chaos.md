# Gossip Chaos Harness
> **Review (2025-09-25):** Synced Gossip Chaos Harness guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The chaos harness exercises gossip under adverse conditions by dropping 15%
of messages and injecting up to 200 ms of jitter. It records convergence time,
asserting that the orphan rate remains below 8% and convergence occurs within
three ticks. Peer identity and bootstrap order can be fixed with `TB_NET_KEY_SEED`
and `TB_PEER_SEED` for reproducible runs. Randomized RPC timeout seeds are
persisted under `target/test-seeds/` so failures can be replayed by setting
`TB_RPC_SEED`.

Tie-break rules prefer higher chain height, then cumulative weight, then the
lexicographically smallest tip hash. A `node/tests/util/fork.rs` fixture injects
divergent blocks for regression coverage.

Run it via `cargo test --test gossip_chaos`.
