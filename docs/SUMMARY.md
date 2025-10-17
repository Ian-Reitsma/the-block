# Summary
> **Review (2025-10-16, evening++)**: `foundation_serialization` now passes its
> full stub-backed test suite. The refreshed `json!` macro handles nested
> literals and identifier keys, binary/TOML fixtures ship handwritten
> serializers, and the `foundation_serde` stub exposes direct primitive visitors
> so tuple decoding no longer panics under FIRST_PARTY_ONLY. Serialization docs
> and guards now treat the facade as fully first party.
> **Review (2025-10-16, midday):** QUIC peer-cert caches now serialize through a sorted peer/provider view so `quic_peer_certs.json` stays byte-identical across runs and guard fixtures. New unit tests lock ordering, history, and rotation counters while `peer_cert_snapshot()` reuses the same sorted iterator to keep CLI/RPC payloads deterministic.
> **Review (2025-10-16, dawn+):** Light-client persistence, snapshots, and chunk serialization now run through deterministic first-party serializers with fixtures (`PERSISTED_STATE_FIXTURE`, `SNAPSHOT_FIXTURE`) and guard-parity tests covering account permutations plus compressed snapshot recovery. Integration coverage drives the in-house `coding::compressor_for("lz77-rle", 4)` path so resume flows remain identical under `FIRST_PARTY_ONLY`.
> **Review (2025-10-14, late evening+++):** Dependency governance now emits a
> structured summary and monitoring coverage. `dependency-check.summary.json`
> ships with every registry run, `tools/xtask` surfaces the parsed verdict,
> release provenance hashes the summary alongside telemetry/metrics artefacts,
> and regenerated Grafana dashboards plus alert rules visualise drift, policy
> status, and snapshot freshness. Integration/release tests enforce the artefact
> contract so CI and operations consume the same signals.
> **Review (2025-10-14, pre-dawn++):** Dependency governance automation now
> includes a reusable CLI runner that emits registry JSON, violations, telemetry
> manifests, and optional snapshots while returning `RunArtifacts` for automation
> and respecting a `TB_DEPENDENCY_REGISTRY_DOC_PATH` override. A new end-to-end
> CLI test drives that runner against the fixture workspace, validating JSON
> payloads, telemetry counters, snapshot output, and manifest listings. Registry
> parsing gained a complex metadata fixture covering optional/git/duplicate
> edges to lock adjacency deduplication and origin detection, and log rotation
> writes now roll back to the original ciphertext if any sled write fails
> mid-run.
> **Review (2025-10-14, late night+):** Dependency registry policy parsing and
> snapshotting run exclusively on the serialization facade. TOML configs flow
> through `foundation_serialization::toml::parse_table`, registry structs map to
> manual JSON `Value`s, and CLI outputs use `json::to_vec_value`, so serde drops
> from the crate while stub-mode tests stay enabled via new regression fixtures
> (including the TOML parser harness).
> **Review (2025-10-14, late night):** Log index key rotation now stages entries
> before writing so failures never leave mixed ciphertext; the suite added an
> atomic rotation regression test and the JSON probe exercises a `LogEntry`
> round-trip to skip cleanly when the stub backend is active. The dependency
> registry CLI drops `cargo_metadata`/`camino`, shells out to `cargo metadata`
> directly, and parses through the first-party JSON facade with unit and
> integration coverage that auto-skip on stub builds.
> **Review (2025-10-14):** `crates/sys` inlines the Linux inotify and BSD/macOS
> kqueue ABIs, removing the crate’s `libc` dependency while runtime’s
> `fs::watch` backend drops the `nix` bridge and reuses Mio registration for all
> platforms. Network sockets now rely solely on Mio helpers, finishing the
> `socket2` removal. Dependency inventories were refreshed to reflect the new
> first-party coverage.
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
> **Review (2025-10-14, midday++):** Dependency registry check mode now emits structured drift diagnostics and telemetry. The new `check` module stages additions, removals, field-level updates, root package churn, and policy deltas before persisting results via `output::write_check_telemetry`, guaranteeing operators can alert on `dependency_registry_check_status`/`dependency_registry_check_counts` even when rotation fails. `cli_end_to_end` gained a check-mode regression that validates the drift narrative and metrics snapshot, and synthetic metadata fixtures now cover target-gated dependencies plus default-member fallbacks so `compute_depths` stays correct across platform-specific graphs.
> **Review (2025-10-12):** Delivered first-party PQ stubs (`crates/pqcrypto_dilithium`, `crates/pqcrypto_kyber`) so `quantum`/`pq` builds compile without crates.io dependencies while preserving deterministic signatures and encapsulations for commit–reveal, wallet, and governance flows. Replaced the external `serde_bytes` crate with `foundation_serialization::serde_bytes`, keeping `#[serde(with = "serde_bytes")]` annotations on exec/read-receipt payloads fully first party, and refreshed the dependency inventory accordingly. Runtime concurrency now routes `join_all`/`select2`/oneshot handling through the shared `crates/foundation_async` facade with a first-party `AtomicWaker`, eliminating the duplicate runtime channel implementation. New integration tests (`crates/foundation_async/tests/futures.rs`) exercise join ordering, select short-circuiting, panic capture, and oneshot cancellation so FIRST_PARTY_ONLY builds lean on in-house scheduling primitives with coverage.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, diagnostics, TUI, TLS, and the PQ facades are live with governance overrides enforced (2025-10-12). Log ingestion/search now rides the sled-backed `log_index` crate with optional SQLite migration for legacy archives (2025-10-14).

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
