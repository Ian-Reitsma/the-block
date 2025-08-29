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
   execution state and remaining timelock.

Both houses must reach quorum before a proposal enters a timelock period,
after which it may be executed on-chain.

Rollback semantics and CLI usage are documented in
[governance_rollback.md](governance_rollback.md). The `gov status` command
exposes rollback-related metrics so operators can verify that gauges reset after
reverts.

## Proposing an Upgrade

1. Draft a feature flag JSON file under `governance/feature_flags/` describing
the change and activation height.
2. Open a PR referencing the proposal. Include motivation, security impact and
link to any specification updates.
3. On merge, operators include the flag file in their configs. When the block
height reaches the activation point, nodes begin enforcing the new rules.

## Handshake Signaling

Peers exchange protocol versions and required feature bits (`0x0004` for fee
routing v2) during handshake. A node will refuse connections from peers
advertising an incompatible protocol version or missing required features.

For more details on badge voting, shard districts and protocol negotiation, see
[AGENTS.md §16 — Vision & Strategy](../AGENTS.md#16-vision-strategy)
and `agents_vision.md`.
