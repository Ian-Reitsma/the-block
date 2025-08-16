# Governance and Upgrade Path

The Block uses service badges to weight votes on protocol changes. Nodes with a
badge may participate in on-chain governance, proposing and approving feature
flags that activate new functionality at designated block heights.

## Proposing an Upgrade

1. Draft a feature flag JSON file under `governance/feature_flags/` describing
the change and activation height.
2. Open a PR referencing the proposal. Include motivation, security impact and
link to any specification updates.
3. On merge, operators include the flag file in their configs. When the block
height reaches the activation point, nodes begin enforcing the new rules.

## Handshake Signaling

Peers exchange protocol versions and enabled feature flags during handshake. A
node will refuse connections from peers advertising an incompatible protocol
version.

For more details on badge voting, shard districts and protocol negotiation, see
`agents_vision.md`.
