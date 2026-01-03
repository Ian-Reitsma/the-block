# Compute-Market Simulation Scenarios
> Compute-market simulations share the same dependency-sovereign stack as the node: runtime, transport, overlay, storage_engine, coding, crypto_suite, and serialization facades run with governance overrides enforced.

Placeholder directory for YAML scenarios exploring admission limits,
quota exhaustion, and governance parameter changes. Each file specifies
buyer/provider workloads and expected rejection rates. See
[Developer Handbook § Formal Methods and Verification](../../docs/developer_handbook.md#formal-methods-and-verification)
for the scenario schema.

Upcoming scenarios should include BlockTorch training jobs (coordinator
postings, deterministic batches, proof overhead targets, and settlement
flows) as described in
[`docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md`](../../docs/ECONOMIC_PHILOSOPHY_AND_GOVERNANCE_ANALYSIS.md#124-distributed-training-architecture-for-the-compute-marketplace).
