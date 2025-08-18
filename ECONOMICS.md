# ECONOMICS — Fee Routing Glossary and Invariants

Pointer: For the project-wide glossary and cross-references, see `AGENTS.md` §14. Keep invariant names (`INV-FEE-01`, `INV-FEE-02`) synchronized between this file and AGENTS.md.

## Glossary

- **Consumer Token (CT)** — unit for high‑velocity transactions.
- **Industrial Token (IT)** — unit for resource‑provision incentives.
- **Fee Selector (ν)** — 2‑bit field mapping `{0 → CT-only, 1 → IT-only, 2 → Split CT∶IT, 3 → Reserved}`.
- **Fee Base (f)** — raw `u64` supplied by the sender; admission enforces `f < 2^63`.
- **Fee Allocation Vector (Δf_miner, Δf_sender)** — ordered pair of per‑token balance deltas applied at commit time.

## Invariant INV-FEE-01 — Supply Neutrality <a id="inv-fee-01"></a>

For every block `B` and token `T ∈ {CT, IT}`:

\[
\sum balances_T(\text{before }B) - \sum balances_T(\text{after }B)
  = \sum f_{T,\text{deducted by senders in }B} - \sum f_{T,\text{credited to miners in }B} = 0.
\]

When `ν = 2`, let `f = 2k + r` with `r ∈ {0,1}`. The decomposition yields `fee_ct = k + r` and `fee_it = k`. Thus `fee_ct + fee_it = 2k + r = f`, preserving total supply even when `f` is odd.

## Invariant INV-FEE-02 — Overflow Safety <a id="inv-fee-02"></a>

For every transaction `tx` and token class `T`:

\[
amount_T + fee_{T}(tx) \leq MAX\_SUPPLY_T \quad \land \quad fee_{T}(tx) \leq 2^{63}-1.
\]

Given the pre‑admission clamp `f < 2^{63}`, the split rules guarantee each `fee_T(tx) ≤ 2^{63}-1`. Any `f ≥ 2^{63}` would produce `fee_T(tx) ≥ 2^{63}` in at least one path, violating the inequality above.

## Edge‑Case Checklist

| Input `(ν,f)`             | Sender Δ(CT,IT) | Miner Δ(CT,IT) | Required Behaviour | Violated Invariant if Mis-handled |
|---------------------------|-----------------|----------------|-------------------|-----------------------------------|
| `f=0,  ν=0`               | `(0,0)`         | `(0,0)`        | No fee deducted    | —                                 |
| `f=1,  ν=2`               | `(-1,0)`        | `(1,0)`        | Split via ceil/floor| INV-FEE-01                         |
| `f=1,  ν=1`               | `(0,-1)`        | `(0,1)`        | Charge IT only     | INV-FEE-01                         |
| `f=2^63−1, ν=0`           | `(-(2^63−1),0)`| `(2^63−1,0)`   | Max legal fee      | INV-FEE-02                         |
| `f=2^63, any ν`           | —               | —              | Reject admission (FeeOverflow) | INV-FEE-02             |
