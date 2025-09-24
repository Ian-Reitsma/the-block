# HTLC-Based Cross-Chain Swaps
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Hash Time Locked Contracts enable atomic exchange across heterogeneous chains. A spender locks funds with a hash of a secret and a timeout. The counterparty reveals the preimage on redemption, allowing the spender to claim funds on the opposite chain. If the timeout elapses, a refund path returns funds to the originator.

```mermaid
sequenceDiagram
    participant A as Chain A
    participant B as Chain B
    A->>A: Create HTLC (hash, timeout)
    B->>B: Mirror HTLC
    A->>B: Reveal preimage
    B->>A: Redeem using preimage
    B->>B: Claim funds
    A->>A: Claim from HTLC
```