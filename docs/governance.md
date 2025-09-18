# Governance and Upgrade Path

The Block uses service badges to weight votes on protocol changes. Nodes with a
badge may participate in on-chain governance, proposing and approving feature
flags that activate new functionality at designated block heights.

Two chambers participate in ratifying upgrades:

- **Operator House** – one vote per high-uptime node.
- **Builder House** – one vote per active core developer.

## Proposal lifecycle

1. Draft a JSON proposal (see `examples/governance/`).
2. Submit with `contract gov submit <file>` to receive an id.
3. Cast votes via `contract gov vote <id> --house ops|builders`.
4. After both houses reach quorum and the timelock elapses, execute with `contract gov exec <id>`.
5. Inspect progress at any time with `contract gov status <id>` which reports vote totals,
   execution state and remaining timelock. Use `contract gov list` to view all proposals
   and their dependencies in a single summary.

Both houses must reach quorum before a proposal enters a timelock period,
after which it may be executed on-chain.

Rollback semantics and CLI usage are documented in
[governance_rollback.md](governance_rollback.md). The `contract gov status` command
exposes rollback-related metrics so operators can verify that gauges reset after
reverts.

### Dependencies and Treasury

Proposals may declare dependencies on previously activated proposals. The
dependency graph is enforced as a DAG, and voting on a proposal is only allowed
once all of its dependencies have activated. This allows staged rollouts and
conflict resolution. Additionally, a governance treasury collects a configurable
percentage of block subsidies in a `TreasuryState`, providing funds for future
initiatives.

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
