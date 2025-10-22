# Summary
> **Review (2025-10-22, late evening):** Bridge incentives now persist through a shared `bridge-types` crate and sled-backed duty/accounting ledgers in `node/src/bridge/mod.rs`. Manual JSON encoders expose the state to RPC/CLI consumers (`bridge.relayer_accounting`, `bridge.duty_log`, `blockctl bridge accounting`, `blockctl bridge duties`), and integration coverage in `node/tests/bridge.rs` plus `node/tests/bridge_incentives.rs` exercises honest/faulty relayers, governance overrides, and challenge penalties under `FIRST_PARTY_ONLY`.
> **Review (2025-10-22, mid-morning+):** CLI wallet tests now snapshot signer
> metadata end-to-end: the `fee_floor_warning` integration suite asserts the
> JSON metadata vector for ready and override previews, and a dedicated
> `wallet_signer_metadata` module covers local, ephemeral, and session signers
> while verifying the auto-bump telemetry event. These tests operate on
> first-party `JsonMap` builders, keeping the suite hermetic without mock RPC
> servers and ensuring the new metadata stays stable for FIRST_PARTY_ONLY runs.
> **Review (2025-10-22, early morning):** Wallet build previews now emit and test
> signer metadata alongside payloads, giving JSON consumers a deterministic view
> of auto-bump, confirmation, ephemeral, and session sender states while
> snapshotting the JSON array for future regressions. Service-badge and
> telemetry commands gained helper-backed unit tests that assert on the
> JSON-RPC envelopes for `verify`/`issue`/`revoke` and `telemetry.configure`,
> keeping the CLI suite entirely first-party. The mobile notification and node
> difficulty examples have been manualized as well, dropping their last
> `foundation_serialization::json!` calls in favour of shared `JsonMap`
> builders so docs tooling mirrors production behaviour. `cargo test
> --manifest-path cli/Cargo.toml` passes with the new metadata assertions.
> **Review (2025-10-21, evening++):** Treasury CLI helpers now power every test
> scenario directly. Lifecycle coverage funds the sled-backed store before
> execution, asserts on typed status transitions, and avoids
> `foundation_serialization::json::to_value`, while remote fetch regression
> tests exercise `combine_treasury_fetch_results` with and without balance
> history to guarantee deterministic JSON assembly. The suite runs to completion
> without `JsonRpcMock` servers, and the node library tests were rerun to green
> to confirm the CLI refactor leaves runtime responders untouched.
> **Review (2025-10-21, mid-morning):** The contract CLI now exposes a shared
> `json_helpers` module that centralizes first-party JSON builders and RPC
> envelope helpers. Compute, service-badge, scheduler, telemetry, identity,
> config, bridge, and TLS commands build payloads through explicit `JsonMap`
> assembly, governance listings serialize through a typed wrapper, and the node
> runtime log sink plus the staking/escrow wallet binary reuse the same helpers.
> This removes the last `foundation_serialization::json!` macros from operator
> tooling while preserving legacy response shapes and deterministic ordering.
> **Review (2025-10-21, pre-dawn):** Governance webhooks now honour
> `GOV_WEBHOOK_URL` regardless of the `telemetry` feature flag, so operators
> receive activation/rollback notifications even on minimal builds. The CLI
> networking stack (`net`, `gateway`, `light_client`, `wallet`) introduces a
> reusable `RpcRequest` helper and explicit `JsonMap` assembly, replacing
> `foundation_serialization::json!` literals with deterministic first-party
> builders; the node’s `net` binary mirrors the change for peer stats, exports,
> and throttle calls. These manual builders keep FIRST_PARTY_ONLY pipelines clean
> while retaining the existing RPC shapes.
> **Review (2025-10-20, near midnight):** Admission now backfills a priority
> tip when callers omit one by subtracting the live base fee from
> `payload.fee`, keeping legacy builders compatible with the lane floor and
> restoring the base-fee regression under FIRST_PARTY_ONLY. Governance
> retuning no longer touches `foundation_serde`: Kalman state snapshots decode
> via `json::Value` and write through a first-party map builder, so the
> inflation pipeline stays entirely on the in-house JSON facade.
> **Review (2025-10-20, late evening):** Canonical transaction payloads now
> serialise exclusively through the cursor helpers. Node `canonical_payload_bytes`
> forwards to `encode_raw_payload`, signed-transaction hashing reuses the manual
> writer, the Python bindings decode via `decode_raw_payload`, and the CLI signs
> by converting into the node struct before calling the same encoder. The
> `foundation_serde` stub is no longer touched during RawTxPayload admission,
> unblocking the base-fee regression under FIRST_PARTY_ONLY.
> **Review (2025-10-20, afternoon++):** Peer metric JSON helpers sort drop and
> handshake maps deterministically, with new unit tests guarding the ordering
> as we phase out bespoke RPC builders. Compute-market RPC endpoints for
> scheduler stats, job requirements, provider hardware, and the settlement audit
> log now construct payloads entirely through first-party `Value` builders, so
> capability snapshots, utilization maps, and audit records no longer rely on
> `json::to_value`. DEX escrow status/release responses serialize payment proofs
> and Merkle roots manually, matching the legacy array layout while keeping the
> entire surface inside the in-house JSON facade.
> **Review (2025-10-20, midday):** Block, transaction, and gossip codecs now
> build their structs through `StructWriter::write_struct` with the new
> `field_u8`/`field_u32` helpers, eliminating hand-counted field totals that
> previously surfaced as `Cursor(UnexpectedEof)` during round-trip tests. RPC
> peer metrics dropped `json::to_value` conversions in favour of deterministic
> builders, so `net.peer_stats_export_all` stays fully on the first-party JSON
> stack. Fresh round-trip coverage exercises the updated writers for block,
> blob-transaction, and gossip payloads under the cursor facade.
> **Review (2025-10-20, morning):** Ledger persistence, mempool rebuild, and
> legacy decode paths now run purely on the `ledger_binary` cursor helpers.
> `MempoolEntryDisk` stores a cached `serialized_size`, startup rebuild consumes
> it before re-encoding, and new tests cover the block/account/emission decoders
> plus the five-field mempool layout so FIRST_PARTY_ONLY runs stay green without
> `binary_codec` fallbacks.
> **Review (2025-10-19, midday):** The node RPC client now builds JSON-RPC
> envelopes through manual `json::Value` maps and parses acknowledgements
> without relying on `foundation_serde` derives. `mempool.stats`,
> `mempool.qos_event`, `stake.role`, and `inflation.params` requests reuse the
> shared builder helpers, so FIRST_PARTY_ONLY builds no longer trigger stub
> panics when issuing or decoding client calls.
> **Review (2025-10-19, early morning):** Gossip and ledger surfaces now emit
> binary payloads exclusively through the in-house cursor helpers. The new
> `net::message` encoder signs/serializes every payload variant (handshake, peer
> rotation, transactions, blob chunks, block/chain broadcasts, reputation
> updates) with comprehensive tests, while `transaction::binary` and
> `block_binary` replace legacy serde/bincode shims for raw payloads, signed
> transactions, blob transactions, and blocks. Cursor-backed regression tests
> cover quantum and non-quantum builds, and DEX/storage manifest fixtures now
> inspect cursor output directly instead of depending on `binary_codec`, keeping
> sled snapshots firmly first party.
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
> **Update (2025-10-20):** Node runtime logging and governance webhooks now serialize via explicit first-party helpers. The CLI log sink assembles stderr JSON and Chrome trace output through `JsonMap` builders, and governance webhooks post typed payloads through the in-house HTTP client (`node/src/bin/node.rs`, `node/src/telemetry.rs`), removing the final `foundation_serialization::json!` dependency from production binaries.
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
