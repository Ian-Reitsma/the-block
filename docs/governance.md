# Governance and Upgrade Path

The Block uses service badges to weight votes on protocol changes. Nodes with a
badge may participate in on-chain governance, proposing and approving feature
flags that activate new functionality at designated block heights.

Two chambers participate in ratifying upgrades:

- **Operator House** – one vote per high-uptime node.
- **Builder House** – one vote per active core developer.

## Shared governance crate

All tooling now consumes the `governance` workspace crate, which mirrors the
state machine shipped in the node binary. The crate re-exports the bicameral
voting scaffolding, `GovStore` sled persistence, release approval workflow, and
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

### GovStore persistence & history artifacts

`GovStore::open` seeds a sled database alongside a `governance/history/`
directory that mirrors every activation. When parameters change,
`persist_param_change` appends JSON rows to
`governance/history/param_changes.json` and, for fee-floor updates, mirrors the
window/percentile pair into `governance/history/fee_floor_policy.json`. DID
revocations land in `did_revocations.json` with the address, reason, epoch, and
wall-clock timestamp captured by `revoke_did`. Release approvals populate
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

> **Update (2025-09-20):** the CLI now lists `start`/`end` windows alongside vote
> totals and execution status, replacing the removed dependency field. Capture the
> new scheduling metadata in explorer dashboards before enabling the `cli`
> feature for production drills.

Operators can stage
rollouts by configuring these windows and the global timelock.
Additionally, a governance treasury collects a configurable percentage of block
subsidies in a `TreasuryState`, providing funds for future initiatives.

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
| `kill_switch_subsidy_reduction` | emergency % cut across all subsidies (12 h timelock) | `subsidy_multiplier{type}` |
| `mempool.fee_floor_window` | number of recent fees sampled when computing the floor | `fee_floor_window_changed_total`, `fee_floor_current` |
| `mempool.fee_floor_percentile` | percentile used for the dynamic fee floor | `fee_floor_window_changed_total`, `fee_floor_current` |
| `scheduler.weight_gossip` | relative ticket weight for gossip scheduler tasks | `scheduler_class_wait_seconds{class="gossip"}` |
| `scheduler.weight_compute` | relative ticket weight for compute scheduler tasks | `scheduler_class_wait_seconds{class="compute"}` |
| `scheduler.weight_storage` | relative ticket weight for storage scheduler tasks | `scheduler_class_wait_seconds{class="storage"}` |

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

## Handshake Signaling

Peers exchange protocol versions and required feature bits (`0x0004` for fee
routing v2) during handshake. A node will refuse connections from peers
advertising an incompatible protocol version or missing required features.

For more details on badge voting, shard districts and protocol negotiation, see
[AGENTS.md §16 — Vision & Strategy](../AGENTS.md#16-vision-strategy)
and `agents_vision.md`.
