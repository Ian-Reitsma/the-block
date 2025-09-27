# Risk Register
> **Review (2025-09-25):** Synced Risk Register guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

| ID | Risk | Owner | Mitigation | Review Date |
|----|------|-------|------------|-------------|
| ECON-01 | Fee algorithm may misallocate rewards | Lead Economist | Formal proof in `formal/fee_v2.fst`; fuzz tests | 2025-01-01 |
| NET-01 | Gossip storm from unbounded peers | Networking Lead | Rate limits and inventory-based gossip | 2025-09-10 |
| SEC-01 | Overflow in miner payout | Security Chair | Add saturating math and overflow tests | 2025-08-28 |
