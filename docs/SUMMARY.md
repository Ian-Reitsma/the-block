# Summary
> **Review (2025-10-12):** Runtime replaced the `crossbeam-deque` scheduler
> with a first-party `WorkQueue` that drives both async tasks and the blocking
> pool while preserving spawn-latency/pending-task telemetry. Added
> `crates/foundation_bigint/tests/arithmetic.rs` so addition, subtraction,
> multiplication, shifting, parsing, and modular exponentiation lock the new
> in-house big-integer engine against deterministic vectors across FIRST_PARTY
> and external configurations.
> **Review (2025-10-12):** Introduced the `foundation_serde` facade and stub
> backend, replicated serde’s trait surface (serializers, deserializers,
> visitors, primitive impls, and value helpers), and taught
> `foundation_serialization` to toggle between external and stub backends via
> features. Workspace manifests now alias `serde` to the facade, and
> `foundation_bigint` replaces the `num-bigint` stack inside `crypto_suite` so
> `FIRST_PARTY_ONLY` builds avoid crates.io big-integer crates; remaining
> `num-traits` edges live behind image/num-* helpers outside the crypto path.
> FIRST_PARTY_ONLY builds compile with
> `cargo check -p foundation_serialization --no-default-features --features
> serde-stub`, and the dependency inventory/guard snapshots were refreshed to
> capture the new facade boundaries.
> **Review (2025-10-12):** Delivered first-party PQ stubs (`crates/pqcrypto_dilithium`, `crates/pqcrypto_kyber`) so `quantum`/`pq` builds compile without crates.io dependencies while preserving deterministic signatures and encapsulations for commit–reveal, wallet, and governance flows. Replaced the external `serde_bytes` crate with `foundation_serialization::serde_bytes`, keeping `#[serde(with = "serde_bytes")]` annotations on exec/read-receipt payloads fully first party, and refreshed the dependency inventory accordingly. Runtime concurrency now routes `join_all`/`select2`/oneshot handling through the shared `crates/foundation_async` facade with a first-party `AtomicWaker`, eliminating the duplicate runtime channel implementation. New integration tests (`crates/foundation_async/tests/futures.rs`) exercise join ordering, select short-circuiting, panic capture, and oneshot cancellation so FIRST_PARTY_ONLY builds lean on in-house scheduling primitives with coverage.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, diagnostics, TUI, TLS, and the PQ facades are live with governance overrides enforced (2025-10-12).

- [Progress Snapshot & Pillar Evidence](progress.md)
- [Dependency Sovereignty Pivot Plan](pivot_dependency_strategy.md)
- [Status, Roadmap & Milestones](roadmap.md)
 
> Consolidation note: Subsystem specs defer to `pivot_dependency_strategy.md` for wrapper-wide policy guidance so individual guides focus on implementation specifics.
- [Telemetry](telemetry.md)
- [Telemetry Operations Runbook](telemetry_ops.md)
- [Networking](networking.md)
  - [Overlay Abstraction](p2p_protocol.md#overlay-abstraction)
  - [Overlay Telemetry & CLI](networking.md#overlay-backend-troubleshooting)
  - [Storage Engine Abstraction](storage.md#0-storage-engine-abstraction)
- [Concurrency](concurrency.md)
- [Gossip](gossip.md)
- [Sharding](sharding.md)
- [Compute Market](compute_market.md)
- [Compute SNARKs](compute_snarks.md)
- [Monitoring & Aggregator Playbooks](monitoring.md)
- [Crypto Suite Migration](crypto_migration.md)
- [Codec & Serialization Guardrails](serialization.md)
- [HTLC Swaps](htlc_swaps.md)
- [Storage Market](storage_market.md)
- [Fee Market](fees.md)
- [VM Gas Model](vm.md)
- [Light Client Stream](light_client_stream.md)
- [Operators](operators/README.md)
  - [Run a Node](operators/run_a_node.md)
  - [Telemetry](operators/telemetry.md)
  - [Incident Playbook](operators/incident_playbook.md)
  - [Upgrade Guide](operators/upgrade.md)
- [Contributing](contributing.md)
- [Development Notes](development.md)
