# Governance and Upgrade Path
> **Review (2025-09-25):** Synced Governance and Upgrade Path guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The Block uses service badges to weight votes on protocol changes. Nodes with a
badge may participate in on-chain governance, proposing and approving feature
flags that activate new functionality at designated block heights.

Two chambers participate in ratifying upgrades:

- **Operator House** – one vote per high-uptime node.
- **Builder House** – one vote per active core developer.

## Shared governance crate

All tooling now consumes the `governance` workspace crate, which mirrors the
state machine shipped in the node binary. The crate re-exports the bicameral
voting scaffolding, `GovStore` first-party sled persistence, release approval workflow, and
parameter registry so SDKs, the CLI, and external services build against the
same API surface as the node. Consumers instantiate a `GovStore`, submit
proposals, and drive activation through the provided `Runtime` facade, which is
backed by a `RuntimeAdapter` implementation that bridges to the host
application. Release submissions validate provenance attestations via the
`ReleaseVerifier` trait, and helper docs in the crate show end-to-end proposal
submission and activation flows.

Migration steps:

1. Add `governance = { path = "../governance" }` to your crate dependencies.
2. Replace `the_block::governance::*` imports with `governance::*` and, when
   driving parameter changes, wrap local runtime hooks in a `RuntimeAdapter`
   implementation.
3. Use `controller::submit_release` with a verifier that taps existing
   attestation keys to share the release quorum checks performed by the node.

The CLI already exercises this path, proving compatibility with the existing
history directories and proposal snapshots.

### Dependency backend governance parameters

Governance now controls dependency backends directly. Proposals may set
`RuntimeBackend`, `TransportProvider`, `StorageEnginePolicy`, `CodingBackend`,
`CryptoSuiteBackend`, and `CodecProfilePolicy` parameters. Each entry maps to the
wrapper crates that ship first-party implementations (`crates/runtime`,
`crates/transport`, `crates/storage_engine`, `crates/coding`, `crates/crypto_suite`,
and `crates/codec`). Policy enforcement occurs during node startup: configuration
files load defaults, the governance store applies overrides, and the runtime selects
backends accordingly. CLI commands (`gov submit dependency-backend`,
`gov vote dependency-backend`, and `gov status dependency-backend`) plus explorer and
RPC payloads expose active selections so operators can verify cluster consensus on
approved providers. Telemetry counters (`runtime_backend_info`,
`transport_provider_connect_total{provider}`, `overlay_backend_active`,
`storage_engine_info`, `coding_backend_selected`, `crypto_suite_backend_active`, and
`codec_profile_active`) tag emitted metrics with governance-sourced labels, and
release provenance scripts refuse to tag artifacts when governance overrides fail the
dependency registry snapshot check.

### GovStore persistence & history artifacts

`GovStore::open` seeds a first-party sled database alongside a `governance/history/`
directory that mirrors every activation. When parameters change,
`persist_param_change` appends JSON rows to
`governance/history/param_changes.json`, mirrors fee-floor updates to
`governance/history/fee_floor_policy.json`, and snapshots dependency policies to
`governance/history/dependency_policy.json`. DID revocations land in
`did_revocations.json` with the address, reason, epoch, and wall-clock
timestamp captured by `revoke_did`. Release approvals populate
`approved_releases` in sled while also persisting signer sets, thresholds, and
install timestamps (`release_installs`) so explorers and dashboards can render
rollout progress. Every activation snapshot is written to
`governance/history/<epoch>.json`, giving operators a time series of runtime
parameters for audit.

Use these artifacts to reconcile CLI output with explorer state and to seed
disaster-recovery drills. For example, copying `param_changes.json` and
`did_revocations.json` between environments lets operators replay policy history
after a catastrophic disk loss.

### Proposal dependency validation

The crate enforces acyclic dependencies via
`governance::proposals::validate_dag`, which runs cycle detection before a
proposal is accepted. Downstream tooling (CLI, SDKs, governance UI) should call
this helper to reject graphs that would deadlock activation. Dependencies are
stored as proposal IDs and evaluated alongside the activation queue; `activate_ready`
only applies proposals whose prerequisites have finished, preventing orphaned
parameter changes from slipping through manual reviews.

### Release quorum tracking and installs

Release votes (`ReleaseVote`/`ReleaseBallot`) now deduplicate signer sets, verify
attestations through pluggable verifiers, and persist successful hashes into the
approved-release sled tree. Each installation is recorded via
`GovStore::record_release_install`, which normalizes timestamps and augments the
history written under `governance/history`. Dashboards can join the stored
install vectors with telemetry (`release_installs_total`) to alert when an
upgrade stalls, and CLI/explorer views render the signer threshold, signatures,
and install cadence from the same store.

## Proposal lifecycle

1. Draft a JSON proposal (see `examples/governance/`).
2. Submit with `contract gov submit <file>` to receive an id.
3. Cast votes via `contract gov vote <id> --house ops|builders`.
4. After both houses reach quorum and the timelock elapses, execute with `contract gov exec <id>`.
5. Inspect progress at any time with `contract gov status <id>` which reports vote totals,
   execution state and remaining timelock. Use `contract gov list` to view all proposals,
   including their configured voting windows.

Both houses must reach quorum before a proposal enters a timelock period,
after which it may be executed on-chain.

Rollback semantics and CLI usage are documented in
[governance_rollback.md](governance_rollback.md). The `contract gov status` command
exposes rollback-related metrics so operators can verify that gauges reset after
reverts.

### Treasury and Scheduling

Proposals track their voting windows explicitly via `start` and `end` timestamps,
which the CLI surfaces when listing the governance queue. Explorer timelines and
telemetry dashboards consume the same fields so operators can monitor concurrent
scheduling windows without relying on the removed dependency metadata.

> **Update (2025-09-25):** the CLI now lists `start`/`end` windows alongside vote
> totals and execution status, replacing the removed dependency field. Capture the
> new scheduling metadata in explorer dashboards before enabling the `cli`
> feature for production drills.

Operators can stage
rollouts by configuring these windows and the global timelock.
Additionally, a governance treasury collects a configurable percentage of block
subsidies in a `TreasuryState`, providing funds for future initiatives. The
`treasury.percent_ct` runtime parameter controls how much of each block’s CT
coinbase is diverted from the miner into the treasury before the reward is paid.

### Treasury Disbursements

Treasury requests are now persisted in both a sled tree (`treasury/disbursements`)
and the legacy JSON file `governance/treasury_disbursements.json`. Balance
snapshots live alongside them in `treasury/balance_history` and
`governance/treasury_balance.json`, ensuring explorer/CLI callers can source
identical history whether they speak directly to sled or consume JSON. Operators
can manage queued payouts with the first-party commands:

```bash
contract gov treasury schedule tb1q... 500000 --memo "grants" --epoch 2048
contract gov treasury schedule tb1q... 100000 --amount-it 25000 --memo "hardware" --epoch 3072
contract gov treasury execute 1 0xdeadbeef
contract gov treasury cancel 2 "policy update"
contract gov treasury list
```

Each action emits structured JSON describing the disbursement ID, destination,
CT and IT amounts, memo, scheduled epoch, and status (`Scheduled`, `Executed`, or
`Cancelled`). Explorer timelines and dashboards should read the same JSON file to
render pending payouts and historical execution trails. Execution helpers record
the supplied transaction hash and timestamps so auditors can reconcile on-chain
movements with governance approval. Every queue/execute/cancel event also
appends a balance snapshot noting the CT/IT deltas, resulting balances, and any
associated disbursement ID, keeping historical accruals in lockstep with the
legacy JSON snapshots.

### Read-acknowledgement anomaly response

Governance stewards the acknowledgement policy because sustained invalid receipt rates can signal hostile gateways or broken client integrations. When dashboards show `read_ack_processed_total{result="invalid_signature"}` or `{result="invalid_privacy"}` rising faster than the `ok` series:

1. Confirm the background worker is still draining the queue by checking the node logs (`read_ack_worker=drain` entries) and verifying the accepted counter is advancing.
2. Pull the latest governance parameters for read-subsidy distribution (`contract gov params show --filter read_subsidy_*`) and ensure no recent proposal redirected subsidies away from the affected roles.
3. Coordinate with gateway operators to rotate client keys or disable offending domains. Use the explorer `/receipts/domain/:id` view to identify which hosts dominate the invalid set.
4. If signatures remain invalid after remediation, schedule an emergency governance vote to pause subsidy minting (`kill_switch_subsidy_reduction` or policy-specific percentage cuts) until the ack stream recovers. Document the action in `governance/history/param_changes.json` for auditors.

These playbooks keep subsidy accounting intact while telemetry, explorer, and CLI surfaces continue to expose per-role payouts for reconciliation.

#### RPC surfaces and CLI fetch workflow

The node exposes the treasury state over first-party JSON-RPC endpoints:

- `gov.treasury.disbursements` returns paginated disbursement records with
  optional status, amount, destination, created-at, status-timestamp, and
  epoch-range filtering so long-range audits can scope the feed without
  replaying the full ledger.
- `gov.treasury.balance` reports the latest balance plus the most recent
  snapshot (including CT and IT totals/deltas).
- `gov.treasury.balance_history` streams historical balance snapshots with
  cursor-based pagination.

These methods back the `contract gov treasury fetch` command, which merges the
three responses into a single JSON document for downstream automation. The CLI
now forwards `--min-amount-ct`, `--max-amount-ct`, `--min-amount-it`,
`--max-amount-it`, `--min-created-at`, `--max-created-at`, `--min-epoch`, and
`--max-epoch` filters when present, and it wraps transport failures with
actionable stderr hints (e.g. connection refused, timeout, malformed endpoint)
so operators can diagnose connectivity issues without diving into logs.

Integration coverage exercises the HTTP dispatcher end-to-end, verifying that
`run_rpc_server` honours pagination, balance snapshots, and the cached
`GovStore`. The metrics aggregator consumes the same sled or JSON payloads; it
falls back to legacy balance schemas that represented numbers as strings and
emits warnings whenever disbursement history exists without accompanying
balance snapshots, helping operators spot persistence regressions before they
affect dashboards. The refreshed aggregator wiring now registers dual-token
gauges for disbursement totals, current balances, and last deltas so dashboards
and alerting inherit CT and IT coverage as soon as governance enables the
activation gate.

## Proposing an Upgrade

1. Draft a feature flag JSON file under `governance/feature_flags/` describing
the change and activation height.
2. Open a PR referencing the proposal. Include motivation, security impact and
link to any specification updates.
3. On merge, operators include the flag file in their configs. When the block
height reaches the activation point, nodes begin enforcing the new rules.

## Runtime Parameters

Governance can adjust several runtime knobs without code changes:

| Parameter Key | Effect | Metrics |
|---------------|--------|---------|
| `fairshare.global_max` | caps aggregate industrial usage as parts-per-million of capacity | `industrial_rejected_total{reason="fairshare"}` |
| `burst.refill_rate_per_s` | rate at which burst buckets replenish | `industrial_rejected_total{reason="burst"}` |
| `inflation.beta_storage_sub_ct` | µCT per byte of storage subsidy | `subsidy_bytes_total{type="storage"}` |
| `treasury.percent_ct` | percentage of each CT coinbase routed into the treasury prior to miner payout | `gov_treasury_balance`, Grafana treasury disbursement row |
| `kill_switch_subsidy_reduction` | emergency % cut across all subsidies (12 h timelock) | `subsidy_multiplier{type}` |
| `mempool.fee_floor_window` | number of recent fees sampled when computing the floor | `fee_floor_window_changed_total`, `fee_floor_current` |
| `mempool.fee_floor_percentile` | percentile used for the dynamic fee floor | `fee_floor_window_changed_total`, `fee_floor_current` |
| `scheduler.weight_gossip` | relative ticket weight for gossip scheduler tasks | `scheduler_class_wait_seconds{class="gossip"}` |
| `scheduler.weight_compute` | relative ticket weight for compute scheduler tasks | `scheduler_class_wait_seconds{class="compute"}` |
| `scheduler.weight_storage` | relative ticket weight for storage scheduler tasks | `scheduler_class_wait_seconds{class="storage"}` |
| `runtime.backend_policy` | bitmask defining the allowed async runtime backends | `gov_dependency_policy_allowed{kind="runtime"}` |
| `transport.provider_policy` | QUIC providers permitted by governance (first entry becomes fallback) | `gov_dependency_policy_allowed{kind="transport"}`, `transport_provider_policy_enforced` |
| `storage.engine_policy` | allowed storage engines and fallback ordering | `gov_dependency_policy_allowed{kind="storage"}`, `storage_engine_policy_enforced` |
| `read_subsidy_viewer_percent` | viewer share of each epoch’s `READ_SUB_CT` | block header `read_sub_viewer_ct`, explorer subsidy snapshots |
| `read_subsidy_host_percent` | host/domain share of `READ_SUB_CT` | block header `read_sub_host_ct`, explorer subsidy snapshots |
| `read_subsidy_hardware_percent` | hardware provider share of `READ_SUB_CT` | block header `read_sub_hardware_ct`, explorer subsidy snapshots |
| `read_subsidy_verifier_percent` | verifier network share of `READ_SUB_CT` | block header `read_sub_verifier_ct`, explorer subsidy snapshots |
| `read_subsidy_liquidity_percent` | liquidity pool share of `READ_SUB_CT` | block header `read_sub_liquidity_ct`, explorer subsidy snapshots |

Tune the percentages as a vector—the node immediately applies the new split to
subsequent acknowledgements while keeping the liquidity bucket as a safety net
for any unclaimed share. Advertising settlements reuse the same ratios, so
campaign spend remains aligned with governance intent.

The `contract` CLI provides shortcuts for parameter management and proposal
crafting:

```bash
contract gov param update mempool.fee_floor_window 512 --state gov.db --epoch 1024
contract gov param update mempool.fee_floor_percentile 70
contract gov submit examples/governance/enable_feature.json
```

Use `contract gov rollback <key>` within the rollback window to revert a change
and restore the previous value. Every activation or rollback appends a record to
`governance/history/fee_floor_policy.json`, increments
`fee_floor_window_changed_total`, and is exposed via the explorer endpoint
`/mempool/fee_floor_policy` for auditing. History entries also capture the
jurisdiction language recorded in the law-enforcement audit trail so operators
can confirm localized notifications.

See [`cli/src/gov.rs`](../cli/src/gov.rs) for additional subcommands that
submit, vote, execute, and roll back proposals. The parameter helper currently
targets the fee-floor keys; other parameters require submitting JSON proposals
manually. DID revocations share the same `GovStore` history; once governance
revokes an address, `identity.anchor` rejects further updates until the entry is
cleared, and the explorer surfaces the state via `/identity/dids/:address`.

### Dependency policy rollout & telemetry

Dependency policies give governance the final say over which runtime, transport,
and storage backends a cluster may activate. Policy proposals encode an
allow-list bitmask; once activated the node runtime normalizes the list,
reconfigures local `NodeConfig`, and emits telemetry (`gov_dependency_policy_allowed`) so
operators can confirm alignment across fleets. Storage and transport policies
apply fallbacks automatically—if a configured engine or provider is disallowed,
the node swaps in the first approved option and logs a warning. Runtime
policies remain advisory but raise telemetry events when a node is outside the
approved set, keeping observability aligned with governance intent. Explorer
routes expose dependency history via `/governance/dependency_policy`, and the
CLI surfaces the same masks when inspecting `gov params`.

Use the bootstrap helper to seed historical records when upgrading an existing
deployment:

```bash
cargo run -p governance --bin bootstrap_dependency_policy -- /var/lib/the-block/state
```

Passing `--force` rewrites existing history snapshots, which is useful when
migrating between staging environments.

## Handshake Signaling

Peers exchange protocol versions and required feature bits (`0x0004` for fee
routing v2) during handshake. A node will refuse connections from peers
advertising an incompatible protocol version or missing required features.

For more details on badge voting, shard districts and protocol negotiation, see
[AGENTS.md §16 — Vision & Strategy](../AGENTS.md#16-vision-strategy)
and `agents_vision.md`.
