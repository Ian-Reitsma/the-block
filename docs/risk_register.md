# Risk Register

| ID | Risk | Owner | Mitigation | Review Date |
|----|------|-------|------------|-------------|
| ECON-01 | Fee algorithm may misallocate rewards | Lead Economist | Formal proof in `formal/fee_v2.fst`; fuzz tests | 2025-01-01 |
| NET-01 | Gossip storm from unbounded peers | Networking Lead | Rate limits and inventory-based gossip | 2024-07-01 |
| SEC-01 | Overflow in miner credit | Security Chair | Add saturating math and overflow tests | 2024-06-15 |
