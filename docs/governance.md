# Governance and Upgrade Path

The Block uses service badges to weight votes on protocol changes. Nodes with a
badge may participate in on-chain governance, proposing and approving feature
flags that activate new functionality at designated block heights.

Two chambers participate in ratifying upgrades:

- **Operator House** – one vote per high-uptime node.
- **Builder House** – one vote per active core developer.

## Proposal lifecycle

1. Draft a JSON proposal (see `examples/governance/`).
2. Submit with `gov submit <file>` to receive an id.
3. Cast votes via `gov vote <id> --house ops|builders`.
4. After both houses reach quorum and the timelock elapses, execute with `gov exec <id>`.
5. Inspect progress at any time with `gov status <id>` which reports vote totals,
   execution state and remaining timelock. Use `gov list` to view all proposals
   and their dependencies in a single summary.

Both houses must reach quorum before a proposal enters a timelock period,
after which it may be executed on-chain.

Rollback semantics and CLI usage are documented in
[governance_rollback.md](governance_rollback.md). The `gov status` command
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

The `gov` helper CLI provides shortcuts for crafting proposals:

```bash
cargo run --bin gov -- SetFairshare 50000 1000
cargo run --bin gov -- SetKillSwitchSubsidyReduction 50
```

See [`node/src/bin/gov.rs`](../node/src/bin/gov.rs)
for additional subcommands that submit, vote, and execute proposals.

## Handshake Signaling

Peers exchange protocol versions and required feature bits (`0x0004` for fee
routing v2) during handshake. A node will refuse connections from peers
advertising an incompatible protocol version or missing required features.

For more details on badge voting, shard districts and protocol negotiation, see
[AGENTS.md §16 — Vision & Strategy](../AGENTS.md#16-vision-strategy)
and `agents_vision.md`.
