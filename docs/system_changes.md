# System-Wide Economic Changes
> **Review (2025-10-10):** Added the `foundation_sqlite`, `foundation_time`, and `foundation_tui` entries, keeping the dependency-sovereignty log current while the codec/telemetry rewrites proceed.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, serialization, SQLite, and TUI facades are live with governance overrides enforced (2025-10-10).

This living document chronicles every deliberate shift in The‑Block's protocol economics and system-wide design. Each section explains the historical context, the exact changes made in code and governance, the expected impact on operators and users, and the trade-offs considered. Future hard forks, reward schedule adjustments, or paradigm pivots must append an entry here so auditors can trace how the chain evolved.

## Transport TLS Asset Wiring (2025-10-10)

### Rationale

- **Drop OpenSSL from QUIC providers:** Quinn and s2n backends relied on ad-hoc
  global state to ingest trust anchors, OCSP staples, and session caches,
  complicating the effort to ship an entirely first-party TLS stack.
- **Consistent provisioning:** Runtime selectors, tests, and operators need a
  single configuration surface to stage TLS material regardless of the active
  QUIC provider.

### Implementation Summary

- Added `transport::TlsSettings` and surfaced it on `transport::Config` so
  callers can provide optional trust anchors, OCSP staples, and TLS resumption
  caches.
- Taught the default transport factory to install (or clear) the configured
  assets when spinning up Quinn or s2n providers, ensuring stale material is
  removed when swapping providers.
- Split `s2n_backend` tests into storage and transport modules to keep targeted
  coverage without namespace collisions when building the crate in isolation.

### Operator & Developer Impact

- Nodes can stage first-party trust stores or OCSP staples without touching
  provider internals, smoothing the migration away from OpenSSL-backed tooling.
- Test harnesses can now exercise TLS asset rotation deterministically by
  injecting ephemeral stores via the shared config structure.

### Migration Notes

- Update any custom transport configuration builders to populate
  `transport::Config::tls` when providing trust anchors or resumption stores.
- Reset helpers (`reset_quinn_tls` / `reset_s2n_tls`) clear global state when a
  provider is not selected, so out-of-band installs should migrate to the shared
  configuration surface.

## In-House QUIC Certificates (2025-10-10)

### Rationale

- **First-party cert material:** The in-house provider previously emitted random
  byte blobs as "certificates", preventing TLS verifiers from authenticating
  peers or extracting public keys for handshake validation.
- **Key continuity:** QUIC callers need deterministic fingerprints and
  verifying keys to compare against advertised peer identities when rotating
  away from Quinn/s2n.

### Implementation Summary

- Taught `foundation_tls` how to recover Ed25519 verifying keys from DER-encoded
  certificates via `ed25519_public_key_from_der` with hardened length parsing.
- Swapped the in-house provider to generate bona fide self-signed Ed25519
  certificates using the shared rotation helpers from `foundation_tls` and
  `foundation_time`, persisting the verifying key alongside the fingerprint.
- Updated `verify_remote_certificate` to reject mismatched public keys rather
  than only hashing the payload, and threaded verifying keys through certificate
  handles so callers can enforce identity checks.
- Adjusted the test harness to provision the new TLS settings and assert the
  stricter verification path.

### Operator & Developer Impact

- QUIC integrations that opt into the in-house backend now surface real
  certificate material, enabling peer identity validation and future TLS
  handshakes without falling back to third-party providers.
- Tests and tooling can trust fingerprint comparisons to reflect the actual
  Ed25519 verifying key embedded in the certificate, closing the gap between
  the in-house backend and production Quinn/s2n deployments.

### Migration Notes

- Existing consumers should cache or distribute the verifying key exposed by
  `CertificateHandle::Inhouse` when comparing peer advertisements.
- The lossy `certificate_from_der` helper still accepts legacy blobs, but any
  handshake using invalid DER will now fail during verification; update fixtures
  accordingly.

## In-House QUIC Handshake Hardening (2025-10-10)

### Rationale

- **Reliability on first-party transport:** The initial UDP + TLS adapter sent
  a single `ClientHello` and trusted best-effort delivery, making in-house QUIC
  materially flakier than the Quinn and s2n providers it is intended to
  replace.
- **Peer identity continuity:** Certificate advertisements only persisted
  fingerprints, forcing higher-level identity checks to guess at the verifying
  key when rotating or auditing peer material.

### Implementation Summary

- Added an exponential retransmission schedule inside the client handshake so
  `ClientHello` frames are resent within the configured timeout and verified by
  an updated server loop that replays cached `ServerHello` payloads and rejects
  stale entries after 30 s.
- Extended the handshake table with explicit TTL tracking, duplicate handling,
  and unit tests that cover cached replies, expiration, and retransmission
  bounds.
- Persisted the Ed25519 verifying key alongside the fingerprint in the JSON
  advertisement store, regenerating cache entries that predate the new schema
  so peers always publish verifiable material.
- Tightened integration coverage to exercise the upgraded handshake path and
  the richer advertisement metadata.

### Operator & Developer Impact

- In-house QUIC connections now enjoy the same retry guarantees as the
  third-party backends, reducing the gap while FIRST_PARTY_ONLY builds phase in
  the native transport.
- Certificate rotation feeds both fingerprints and verifying keys through the
  on-disk advertisement cache, simplifying peer validation across node, CLI,
  and explorer surfaces.

### Migration Notes

- Older advertisement files lacking a verifying key are automatically
  regenerated on load, but operators should verify that distribution pipelines
  and dashboards consume the new field when auditing peer identity.
- The new retransmission schedule honours the existing handshake timeout; tune
  `handshake_timeout` in `config/quic.toml` if deployments relied on the
  previous best-effort behaviour.

## First-Party SQLite Facade (2025-10-10)

### Rationale

- **Dependency sovereignty:** Explorer, CLI, and log/indexer tooling depended
  directly on `rusqlite`, preventing `FIRST_PARTY_ONLY=1` builds from compiling
  and complicating efforts to stub or replace the backend.
- **Unified ergonomics:** Ad-hoc parameter macros and per-tool helpers made it
  easy to drift between positional vs. named parameters and inconsistent error
  handling; a shared facade normalises values, parameters, and optional
  backends.

### Implementation Summary

- Added `crates/foundation_sqlite` exporting `Connection`, `Statement`, `Row`,
  `params!`/`params_from_iter!`, and a lightweight `Value` enum covering the
  rusqlite types we use today.
- Default builds enable the `rusqlite-backend` feature, delegating to the
  existing engine while unifying parameter conversion and query helpers.
- Introduced a first-party `FromValue` decoding trait plus
  `ValueConversionError`, letting the facade translate rows without depending on
  `rusqlite::types::FromSql` so stub and future native engines share identical
  call sites.
- `foundation_sqlite` exposes a stub backend when the feature is disabled,
  returning `backend_unavailable` errors so `FIRST_PARTY_ONLY=1 cargo check`
  surfaces missing implementations without pulling third-party code.
- Migrated explorer query helpers, the CLI `logs` command, and the
  indexer/log-indexer tooling to call the facade (including the new
  `query_map` collector) instead of `rusqlite` directly.

### Operator & Developer Impact

- Tooling builds continue to work with SQLite when the default feature is
  enabled, while first-party-only builds now fail fast with clear errors rather
  than missing symbols.
- Shared parameter/value handling removes subtle differences between tools,
  making it easier to audit SQL statements and extend coverage.
- Future work can swap the backend (or add an embedded engine) inside
  `foundation_sqlite` without touching downstream crates.

### Migration Notes

- Downstream tooling must depend on `foundation_sqlite` instead of `rusqlite`.
- Keep the `rusqlite-backend` feature enabled for production until the in-house
  engine lands; tests can exercise the stub by setting `FIRST_PARTY_ONLY=1` or
  disabling the feature.
- Follow-up work will replace the stub with a native engine so
  `FIRST_PARTY_ONLY=1` builds succeed end-to-end.

## Foundation Time Facade (2025-10-10)

### Rationale

- **Timestamp determinism:** Storage repair logging, S3 snapshot signing, and
  transport certificate rotation all encoded ad-hoc `time` crate calls with
  inconsistent formatting and error handling.
- **First-party builds:** The direct dependency on the upstream `time` crate
  blocked `FIRST_PARTY_ONLY=1` builds and complicated efforts to audit
  formatting changes across runtime and tooling.

### Implementation Summary

- Added `crates/foundation_time` with an in-house `UtcDateTime`/`Duration`
  implementation, deterministic calendar math, and helpers to emit ISO-8601 and
  AWS-style compact timestamps.
- Replaced metrics aggregator S3 signing, storage repair file naming, and the
  QUIC certificate generator with the new facade so runtime/tooling code no
  longer imports `time` directly.
- Landed the first-party `foundation_tls` certificate builder so QUIC rotation
  and test tooling construct Ed25519 X.509 certificates without `rcgen`, using
  facade validity windows end-to-end.

### Operator & Developer Impact

- Timestamp formatting is now consistent across runtime and tooling, reducing
  the odds of drift between logging, signing, and certificate validity windows.
- `FIRST_PARTY_ONLY` builds can compile these surfaces without linking the
  upstream crate; the QUIC stack now signs certificates exclusively through the
  foundation TLS facade.
- Future features (e.g., governance snapshot formatting or log timestamp
  normalization) can extend the facade without reintroducing external
  dependencies.

## Foundation TUI Facade (2025-10-10)

### Rationale

- **Dependency parity:** The node's networking CLI relied on the third-party
  `colored` crate for ANSI output, blocking `FIRST_PARTY_ONLY=1` builds and
  preventing consistent colour policies across tooling.
- **Operator control:** Colour output needed to respect environment overrides
  (`NO_COLOR`, `CLICOLOR`) and TTY detection without depending on crates we aim
  to remove from production builds.

### Implementation Summary

- Added `crates/foundation_tui` with ANSI colour helpers, a `Colorize` trait,
  and environment-aware detection that honours `TB_COLOR`, `NO_COLOR`, and
  terminal detection via `sys::tty`.
- Swapped the node networking CLI (`node/src/bin/net.rs`) to call the new
  helpers, removing the direct `colored` dependency from the workspace.
- Extended `sys::tty` with `stdout_is_terminal`/`stderr_is_terminal`/`stdin_is_terminal`
  helpers so other tooling can reuse the detection logic without reimplementing
  platform-specific checks.

### Operator & Developer Impact

- CLI colour output is now consistent across platforms, respects operator
  overrides, and no longer depends on crates.io packages.
- `FIRST_PARTY_ONLY` builds compile without the `colored` crate while keeping
  familiar `line.red()` ergonomics for downstream tooling.
- Additional styling (bold, underline, background colours) can be added to the
  facade without reintroducing external dependencies.

## Foundation Unicode Normalizer (2025-10-10)

### Rationale

- **First-party input hygiene:** Handle registration, DID validation, and CLI
  helpers previously called into ICU via `icu_normalizer`, blocking
  `FIRST_PARTY_ONLY=1` builds and inflating the dependency surface with large
  Unicode data tables.
- **Deterministic behaviour:** The team needs a predictable, auditable
  normalizer with a clear ASCII fast-path so operator tooling and governance
  flows can agree on canonical forms without relying on opaque upstream tables.

### Implementation Summary

- Introduced `crates/foundation_unicode` with an `nfkc` normalizer that
  short-circuits ASCII inputs, provides compatibility mappings for common
  compatibility characters, and exposes accuracy flags for non-ASCII fallbacks.
- Swapped the node handle registry and integration tests to the facade,
  removing the `icu_normalizer` and `icu_normalizer_data` crates from the
  workspace.
- Documented the facade in the dependency audit so future Unicode work can
  extend the mapping tables without reintroducing third-party code.

### Operator & Developer Impact

- Handle normalisation is now controlled entirely by first-party code; future
  tweaks can land alongside governance decisions instead of waiting on ICU
  updates.
- `FIRST_PARTY_ONLY` builds link the light-weight facade instead of the ICU
  ecosystem, dramatically shrinking the dependency tree for identity tooling.
- The accuracy flag allows downstream callers to detect when non-ASCII fallback
  mappings are used and add additional validation as needed.

## Xtask Git Diff Rewrite (2025-10-10)

### Rationale

- **Remove libgit2 stack:** The `xtask summary` helper depended on the
  third-party `git2` bindings which pulled in libgit2, `url`, `idna`, and the
  ICU normalization crates even after the runtime/CLI migrations, keeping
  `FIRST_PARTY_ONLY=1` builds from linking cleanly.
- **Stabilise tooling behaviour:** Shelling out to the git CLI mirrors the
  commands operators and CI already run, avoids binding-specific corner cases,
  and dramatically shrinks the transitive dependency graph for release checks.

### Implementation Summary

- Replaced the libgit2-backed diff logic with thin wrappers around `git
  rev-parse` and `git diff --patch`, preserving the JSON summary output while
  leaning on the existing CLI.
- Dropped the `git2` dependency from `tools/xtask`, allowing the workspace to
  remove the `url`/`idna_adapter`/ICU stack that the bindings required.
- Updated the first-party manifest and dependency snapshot so `FIRST_PARTY_ONLY`
  guards now pass without whitelisting libgit2.

### Operator & Developer Impact

- Developer tooling no longer links against libgit2, closing another gap on the
  path to all-first-party builds.
- CI summary jobs use the same git CLI output developers see locally, reducing
  surprises when reviewers validate PR summaries or dependency guard output.

## Wrapper Telemetry Integration (2025-09-25)

### Rationale

- **Unified observability:** Runtime, transport, overlay, storage engine, coding, codec, and crypto backends now surface consistent gauges/counters so operators can correlate incidents with dependency switches without grepping logs.
- **Governance evidence:** Backend selections and dependency policy violations are visible to voters before approving rollouts or escalations.
- **CLI and automation:** On-call engineers can fetch the exact wrapper mix from production fleets via a single CLI command.

### Implementation Summary

- Extended `node/src/telemetry.rs` with per-wrapper gauges (`runtime_backend_info`, `transport_provider_connect_total{provider}`, `overlay_backend_active`, `storage_engine_backend_info`, `coding_backend_info`, `codec_serialize_fail_total{profile}`, `crypto_suite_signature_fail_total{backend}`) plus size/failure histograms where applicable.
- Added wrapper snapshots to `metrics-aggregator`, exposing a REST `/wrappers` endpoint, schema docs in `monitoring/metrics.json`, and Grafana dashboards that chart backend selections, failure rates, and policy violation gauges across operator/dev/telemetry views.
- Landed a `contract-cli system dependencies` subcommand that queries the aggregator and formats wrapper status (provider name, version, commit hash, policy tier) for on-call debugging and change management.
- Wired the dependency registry tooling to emit a runtime telemetry `dependency_policy_violation` gauge, enabling alerts when policy drift appears.

### Operator & Governance Impact

- Dashboards and CLI output include provider/codec/crypto labels, making phased rollouts auditable during change windows.
- Governance proposals gain concrete evidence before ratifying backend swaps or remediating policy drift; registry snapshots remain part of release artifacts.
- Runbooks now reference wrapper metrics when diagnosing network incidents or signing off on dependency simulations.

### Next Steps

- Extend storage migration tooling so RocksDB↔sled transitions can be rehearsed alongside wrapper telemetry.
- Feed wrapper summaries into the planned dependency fault simulation harness to rehearse provider outages under controlled chaos scenarios.

## Dependency Sovereignty Pivot (2025-09-23)

### Rationale

- **Risk management:** The node relied on 800+ third-party crates spanning runtime,
  transport, storage, coding, crypto, and serialization. A surprise upstream
  change could invalidate safety guarantees or stall releases.
- **Operational control:** Wrapping these surfaces in first-party crates lets
  governance gate backend selection, run fault drills, and schedule replacements
  without pleading for upstream releases.
- **Observability & trust:** Telemetry, CLI, and RPC endpoints now report active
  providers (runtime, transport, overlay, storage engine, codec, crypto) so
  operators can audit rollouts and correlate incidents with backend switches.

### Implementation Summary

- Formalised the pivot plan in [`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md)
  with 20 tracked phases covering registry, tooling, wrappers, governance,
  telemetry, and simulation milestones.
- Delivered the dependency registry, CI gating, runtime wrapper, runtime
  adoption linting, QUIC transport abstraction, provider introspection,
  release-time vendor syncs, and documentation updates as completed phases.
- Inserted a uniform review banner across the documentation set referencing the
  pivot date so future audits can confirm alignment.

### Operator & Governance Impact

- Operators must reference wrapper crates rather than upstream APIs and consult
  the registry before accepting dependency changes.
- Governance proposals will inherit new parameter families to approve backend
  selections; telemetry dashboards already surface the metadata required to
  evaluate those votes.
- Release managers must include registry snapshots and vendor tree hashes with
  every tagged build; CI now fails if policy drift is detected.

### What’s Next

- Ship the overlay, storage-engine, coding, crypto, and codec abstractions,
  then extend telemetry/governance hooks across each wrapper.
- Build the dependency fault simulation harness so fallbacks can be rehearsed in
  staging before enabling on production.
- Migrate wallet, explorer, and mobile clients onto the new abstractions as they
  land, keeping documentation in sync with the pivot guide.

## QUIC Transport Abstraction (2025-09-23)

### Rationale for the Trait Layer

- **Provider neutrality:** The node previously called Quinn APIs directly and carried an optional s2n path. Abstracting both behind `crates/transport` lets governance swap providers or inject mocks without forking networking code.
- **Deterministic testing:** Integration suites can now supply in-memory providers implementing the shared traits, delivering deterministic handshake behaviour for fuzzers and chaos harnesses.
- **Telemetry parity:** Handshake callbacks, latency metrics, and certificate rotation counters now originate from a common interface so dashboards remain consistent regardless of backend.

### Implementation Summary

- Introduced `crates/transport` with `QuicListener`, `QuicConnector`, and `CertificateStore` traits, plus capability enums consumed by the handshake layer.
- Moved Quinn logic into `crates/transport/src/quinn_backend.rs` with pooled connections, retry helpers, and replaceable telemetry callbacks.
- Ported the s2n implementation into `crates/transport/src/s2n_backend.rs`, wrapping builders in `Arc`, sharing certificate caches, and exposing provider IDs.
- Added a `ProviderRegistry` that selects backends from `config/quic.toml`, surfaces provider metadata to `p2p::handshake`, and emits `quic_provider_connect_total{provider}` telemetry.
- Updated CLI/RPC surfaces to display provider identifiers, rotation timestamps, and fingerprint history sourced from the shared certificate store.

### Operator Impact

- Configuration lives in `config/quic.toml`; reloads rebuild providers without restarting the node.
- Certificate caches are partitioned by provider so migration between Quinn and s2n retains history.
- Telemetry dashboards can segment connection successes/failures by provider, highlighting regressions during phased rollouts.

### Testing & Tooling

- `node/tests/net_quic.rs` exercises both providers via parameterised harnesses, while mocks cover retry loops.
- CLI commands (`blockctl net quic history`, `blockctl net quic stats`, `blockctl net quic rotate`) expose provider metadata for on-call triage.
- Chaos suites reuse the same trait interfaces, ensuring packet-loss drills and fuzz targets remain backend agnostic.

## CT Subsidy Unification (2024)

The network now mints every work-based reward directly in CT. Early devnets experimented with an auxiliary reimbursement ledger, but governance retired that approach in favour of a single, auditable subsidy store that spans storage, read delivery, and compute throughput.

### Rationale for the Switch
- **Operational Simplicity:** A unified CT ledger eliminates balance juggling, decay curves, and swap mechanics.
- **Transparent Accounting:** Subsidy flows reconcile with standard wallets, easing audits and financial reporting.
- **Predictable UX:** Users can provision gateways or upload content with a plain CT wallet—no staging balances or side ledgers.
- **Direct Slashing:** Burning CT on faults or policy violations instantly reduces circulating supply without custom settlement paths.

### Implementation Summary
- Removed the auxiliary reimbursement plumbing and its RPC surfaces, consolidating rewards into the CT subsidy store.
- Introduced global subsidy multipliers `beta`, `gamma`, `kappa`, and `lambda` for storage, read delivery, CPU, and bytes out. These values live in governance parameters and can be hot-tuned.
- Added a rent-escrow mechanism: every stored byte locks `rent_rate_ct_per_byte` CT, refunding 90 % on deletion or expiry while burning 10 % as wear-and-tear.
- Reworked coinbase generation so each block mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` alongside the decaying base reward.
- Redirected the former reimbursement penalty paths to explicit CT burns, ensuring punitive actions reduce circulating supply.

Changes shipped behind feature flags with migration scripts (such as `scripts/purge_legacy_ledger.sh` and updated genesis templates) so operators could replay devnet ledgers and confirm balances and stake weights matched across the switch. Historical blocks remain valid; the new fields simply appear as zero before activation.

### Impact on Operators
- Rewards arrive entirely in liquid CT.
- Subsidy income depends on verifiable work: bytes stored, bytes served with `ReadAck`, and measured compute. Stake bonds still back service roles, and slashing burns CT directly from provider balances.
- Monitoring requires watching `subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, and rent-escrow gauges. Operators should also track `inflation.params` to observe multiplier retunes.

Archive `governance/history` to maintain a local audit trail of multiplier votes and kill-switch activations. During the first epoch after upgrade, double-check that telemetry exposes the new subsidy and rent-escrow metrics; a missing gauge usually indicates lingering legacy configuration files or dashboard panels.

### Impact on Users
- Uploads, hosting, and dynamic requests work with standard CT wallets. No staging balances or alternate instruments are required.
- Reads remain free; the cost is socialized via block-level inflation rather than per-request fees. Users only see standard rate limits if they abuse the service.

Wallet interfaces display the refundable rent deposit when uploading data and automatically return 90 % on deletion, making the lifecycle visible to non-technical users.

### Governance and Telemetry
Governance manages the subsidy dial through `inflation.params`, which exposes the five parameters:
```
 beta_storage_sub_ct
 gamma_read_sub_ct
 kappa_cpu_sub_ct
 lambda_bytes_out_sub_ct
 rent_rate_ct_per_byte
```
An accompanying emergency knob `kill_switch_subsidy_reduction` can downscale all subsidies by a voted percentage. Every retune or kill‑switch activation must append an entry to `governance/history` and emits telemetry events for on-chain tracing.

The kill switch follows a 12‑hour timelock once activated, giving operators a grace window to adjust expectations. Telemetry labels multiplier changes with `reason="retune"` or `reason="kill_switch"` so dashboards can plot long-term trends and correlate them with network incidents.

### Reward Formula Reference
The subsidy multipliers are recomputed each epoch using the canonical formula:
```
multiplier_x = (ϕ_x · I_target · S / 365) / (U_x / epoch_seconds)
```
where `S` is circulating CT supply, `I_target` is the annual inflation ceiling (currently 2 %), `ϕ_x` is the inflation share allocated to class `x`, and `U_x` is last epoch's utilization metric. Each multiplier is clamped to ±15 % of its prior value, doubling only if `U_x` was effectively zero to avoid divide-by-zero blow-ups. This dynamic retuning ensures inflation stays within bounds while rewards scale with real work.

### Pros and Cons
| Aspect | Legacy Reimbursement Ledger | Unified CT Subsidy Model |
|-------|-----------------------------|--------------------------|
| Operator payouts | Separate balance with bespoke decay | Liquid CT every block |
| UX for new users | Required staging an auxiliary balance | Wallet works immediately |
| Governance surface | Multiple mint/decay levers | Simple multiplier votes |
| Economic transparency | Harder to audit total issuance | Inflation capped ≤2 % with public multipliers |
| Regulatory posture | Additional instrument to justify | Single-token utility system with CT sub-ledgers |

### Migration Notes
Devnet operators should run `scripts/purge_legacy_ledger.sh` to wipe obsolete reimbursement data and regenerate genesis files without the legacy balance field. Faucet scripts now dispense CT. Operators must verify `inflation.params` after upgrade and ensure no deprecated configuration keys persist in configs or dashboards.

### Future Entries
Subsequent economic shifts—such as changing the rent refund ratio, altering subsidy shares, or introducing new service roles—must document their motivation, implementation, and impact in a new dated section below. This file serves as the canonical audit log for all system-wide model changes.

## Durable Compute Settlement Ledger (2025-09-21)

### Rationale for Persistence & Dual-Ledger Accounting

- **Crash resilience:** The in-memory compute settlement shim dropped balances on restart. Persisting CT flows (with legacy industrial columns retained for tooling) in RocksDB guarantees recovery, even if the node or process exits unexpectedly.
- **Anchored accountability:** Governance required an auditable trail that explorers, operators, and regulators can replay. Recording sequences, timestamps, and anchors ensures receipts reconcile with the global ledger.
- **Ledger clarity:** Providers and buyers need to understand CT balances after every job. Persisting the ledger avoids race conditions when reconstructing balances from mempool traces and keeps the legacy industrial column available for regression tooling.

### Implementation Summary

- `Settlement::init` now opens (or creates) `compute_settlement.db` inside the configured settlement directory, wiring sled-style helpers that load or default each sub-tree (`ledger_ct`, `ledger_it`, `metadata`, `audit_log`, `recent_roots`, `next_seq`). Test builds without the legacy `storage-rocksdb` toggle transparently fall back to an ephemeral directory while production deployments rely on the in-house engine.
- Every accrual, refund, or penalty updates both the in-memory ledger and the persisted state via `persist_all`, bumping a monotonic sequence and recomputing the Merkle root (`compute_root`).
- `Settlement::shutdown` always calls `persist_all` on the active state and flushes RocksDB handles before dropping them, ensuring integration harnesses (and crash recovery drills) see fully durable CT balances (with zeroed industrial fields) even if the node exits between accruals.
- `Settlement::submit_anchor` hashes submitted receipts, records the anchor in `metadata.last_anchor_hex`, pushes a marker into the audit deque, and appends a JSON line to the on-disk audit log through `state::append_audit`.
- Activation metadata (`metadata.armed_requested_height`, `metadata.armed_delay`, `metadata.last_cancel_reason`) captures the reason for every transition between `DryRun`, `Armed`, and `Real` modes. `Settlement::arm`, `cancel_arm`, and `back_to_dry_run` persist these fields immediately and emit telemetry via `SETTLE_MODE_CHANGE_TOTAL{state}`.
- Telemetry counters `SETTLE_APPLIED_TOTAL`, `SETTLE_FAILED_TOTAL{reason}`, `SLASHING_BURN_CT_TOTAL`, and `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` expose a live view of accruals, refunds, and penalties. Dashboards can alert on stalled anchors or repeated SLA violations.
- RPC endpoints `compute_market.provider_balances`, `compute_market.audit`, and `compute_market.recent_roots` serialize the persisted data so the CLI and explorer can render provider balances, audit trails, and continuity proofs. Integration coverage lives in `node/tests/compute_settlement.rs`, `cli/tests/compute.rs`, and `explorer/tests/compute_settlement.rs`.

### Operational Impact

- **Operators** should monitor the new RPCs and runtime telemetry counters to ensure balances drift as expected, anchors land on schedule, and SLA burns are visible. Automate backups of `compute_settlement.db` alongside other state directories.
- **Explorers and auditors** can subscribe to the audit feed, correlate sequence numbers with Merkle roots, and flag any divergence between local mirrors and the node-provided anchors.
- **Governance and finance** teams gain deterministic evidence of CT burns, refunds, and payouts, unblocking treasury reconciliation and upcoming SLA enforcement proposals.

### Migration Notes

- Nodes upgrading from the in-memory shim should point `settlement_dir` (or the default data directory) at persistent storage before enabling `Real` mode. The first startup migrates balances into RocksDB with a zeroed sequence.
- Automation that previously scraped in-process metrics must switch to the RPC surfaces described above. CLI invocations use the default build; enable the optional `sqlite-migration` feature only when importing legacy SQLite snapshots before returning to the minimal configuration.
- Backups should include `compute_settlement.db` and the `audit.log` file written by `state::append_audit` so post-incident reviews retain both ledger state and anchor evidence.
## Unicode Handle Telemetry and CLI Surfacing (2025-10-10)

### Rationale

- **Dependency sovereignty:** Identity normalization now runs entirely through
  the first-party `foundation_unicode` facade. Latin-1 and Greek letters map to
  ASCII fallbacks so operators no longer depend on ICU tables to register common
  names.
- **Operational visibility:** Registrations now emit
  `identity_handle_normalization_total{accuracy}` so clusters can quantify how
  many handles relied on approximate transliteration and adjust onboarding flows
  accordingly.
- **Operator tooling:** The CLI gained `contract identity register|resolve|normalize`
  commands that show both local and remote normalization results, ensuring human
  operators can detect mismatches before the registry persists a handle.

### Implementation Summary

- Added transliteration tables for accented Latin-1 and Greek characters to the
  Unicode facade. The registry records `NormalizationAccuracy` alongside each
  registration and propagates it through the RPC layer.
- Instrumented the registry with the `identity_handle_normalization_total`
  counter and surfaced accuracy in the RPC response schema.
- Built a dedicated CLI identity module that displays accuracy labels and warns
  when the node accepted an approximate normalization.
- Extended the CLI `identity register` subcommand with an optional
  `--pq-pubkey` flag gated behind the `pq-crypto`/`quantum` features so Dilithium
  registrants can forward their post-quantum key material alongside Ed25519
  payloads.

### Operational Impact

- **Operators** should watch the new counter for spikes in approximate
  normalizations and coach users toward handles that normalize exactly.
- **Support tooling** can invoke the CLI’s `identity normalize` command to audit
  handles offline and reproduce the registry’s transliteration decisions.

## Deterministic TLS Rotation Plans (2025-10-10)

### Rationale

- **Pre-computable rotations:** QUIC and s2n listeners now schedule certificates
  via a deterministic `RotationPolicy`, allowing rotation daemons to prepare
  leaf certificates in advance without relying on randomness.
- **Chain issuance:** The transport layer can bind QUIC listeners with complete
  CA chains, unblocking deployments that terminate on intermediate CAs or need
  to present both leaf and issuer certificates during handshake.
- **Interoperability tests:** Integration suites verify both Quinn and s2n
  providers against CA-signed paths using the new builder, guarding against
  regressions as we extend the TLS facade.

### Implementation Summary

- Introduced `RotationPolicy`/`RotationPlan` in `foundation_tls` and wired the
  certificate builders to derive validity windows and serial numbers from a
  deterministic schedule.
- Updated QUIC and s2n backends to consume rotation plans instead of random
  serials and to expose helpers for installing certificate chains.
- Added CA-signed integration tests for both providers and exposed a listener
  helper (`listen_with_chain`) so nodes can bind endpoints with full chains.
- Implemented provider-specific `ListenerHandle::as_*`/`into_*` helpers for
  Quinn, s2n-quic, and in-house backends, and taught `listen_with_chain` to
  borrow certificate slices, eliminating unnecessary vector clones when
  installing large chains in tests or runtime wiring.

### Operational Impact

- **Rotation jobs** can reuse the shared policy to stage certificates ahead of
  time and coordinate rollouts across multiple nodes.
- **Deployments** issuing certificates from internal CAs can feed chain
  artifacts directly into the QUIC adapter without patching the transport
  crate.

## In-house QUIC Handshake Skeleton (2025-10-10)

### Rationale

- **Dependency sovereignty:** Replaces the placeholder `inhouse_backend` with
  a first-party UDP + TLS handshake so transport builds no longer rely on
  Quinn/s2n just to smoke-test the in-house provider.
- **Certificate fidelity:** Shares the `foundation_tls` certificate helpers so
  the in-house backend generates/verifies the same Ed25519 material used by
  external providers, keeping CLI/RPC validation paths consistent.
- **Stateful advertising:** Persists listener advertisements and rotation data
  through a dedicated certificate store, allowing nodes to reload fingerprints
  without piping through third-party JSON codecs.

### Implementation Summary

- Introduced `crates/transport/src/inhouse/` with `adapter.rs`,
  `messages.rs`, `certificate.rs`, and `store.rs` implementing the UDP
  handshake, message encoding, certificate generation, and JSON-backed
  advertisement persistence.
- Updated the transport registry (`crates/transport/src/lib.rs`) to load the
  new module, pass handshake timeouts through `Config`, and surface
  provider-specific helpers via `ListenerHandle::as_inhouse`.
- Replaced the legacy integration tests with
  `crates/transport/tests/inhouse.rs`, covering successful round trips,
  certificate mismatches, metadata introspection, and certificate-store
  rotation.

### Operational Impact

- **Operators** can now exercise the in-house transport end-to-end without
  enabling third-party QUIC crates, paving the way for
  `FIRST_PARTY_ONLY=1` transport builds.
- **Certificate tooling** can rely on the shared store to inspect fingerprints
  and issued-at timestamps when debugging node rotations.
- **Telemetry** continues to surface handshake success/failure via the
  existing callbacks, ensuring dashboards reflect the in-house backend just
  like Quinn and s2n.

## Default Transport Provider Switch (2025-10-10)

### Rationale

- **First-party by default:** With the in-house UDP/TLS adapter now carrying
  parity coverage, the transport configuration should prefer it automatically
  so new nodes no longer reach for Quinn before custom code.
- **Build readiness:** Bundling the `inhouse` feature into the node’s
  transport dependency keeps the provider available in every QUIC-enabled
  build while the third-party stacks remain compiled for comparison tests.

### Implementation Summary

- Updated `transport::Config::default` to resolve the preferred provider at
  compile time, prioritising the in-house backend when it is enabled.
- Enabled the `inhouse` feature on the node’s transport dependency so QUIC
  builds ship the first-party implementation alongside legacy providers.

### Operational Impact

- **Node boot defaults** now point at the in-house provider whenever it is
  compiled, reducing manual configuration for operators embracing
  `FIRST_PARTY_ONLY` builds.
- **CI configurations** maintain Quinn and s2n support for parity suites, but
  the runtime registry starts with the custom adapter, accelerating
  first-party rollout.
