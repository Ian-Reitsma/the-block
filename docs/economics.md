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
  = \sum f_{T,\text{deducted by senders in }B} - \sum f_{T,\text{awarded to miners in }B} = 0.
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

## Simulation Harness

The `sim/` crate models inflation, demand, liquidity, and backlog dynamics. Running
`cargo run -p tb-sim --example governance_tuning` writes per-step KPIs to
`/tmp/gov_tuning.csv` with the following columns:

- `inflation_rate` – current annualised inflation
- `sell_coverage` – liquidity divided by backlog
- `readiness` – inverse backlog indicator

These scenarios help governance evaluate issuance and demand tuning.

## Compute-Backed Money

Compute-backed tokens (CBTs) redeem for compute units at a protocol-defined
curve. A linear redeem curve priced off the marketplace median ensures large
burns incur a premium, slowing depletion of the fee-funded backstop. Fees from
compute jobs replenish the backstop reserve.

The `ComputeToken` prototype models this flow. Tokens represent compute units
and redeem against a `RedeemCurve` while debiting a shared `Backstop` reserve.
See `node/tests/compute_cbt.rs` for an instant-app style settlement example.
## Epoch Retuning Formula

The chain mints storage, read, and compute subsidies in CT. At each epoch the multipliers
are recalibrated to keep annual inflation under 2 %:

\[
\text{multiplier}_x = \frac{\phi_x \cdot I_{\text{target}} \cdot S / 365}{U_x / \text{epoch\_secs}}
\]

Where:

- \(S\) – current CT supply
- \(I_{\text{target}} = 0.02\) (2 % yearly)
- \(\phi_x\) – fraction of the inflation budget for class \(x\)
- \(U_x\) – observed utilization last epoch (bytes or ms)

Results are clamped to ±15 % of the prior multiplier to avoid oscillation. If
`U_x` is near zero, the previous multiplier doubles to keep incentives from
stalling.

```rust
fn retune_multipliers(state: &ChainState, stats: &UtilStats) {
    let s = state.ct_supply();
    let epoch_secs = stats.epoch_secs as f64;
    let target = 0.02;
    let yr_secs = 31_536_000.0;
    let calc = |util: f64, phi: f64, prev: f64| {
        let mut next = if util < 1.0 {
            prev * 2.0
        } else {
            let yearly = util * (yr_secs / epoch_secs);
            (phi * target * s) / yearly
        };
        next = next.clamp(prev * 0.85, prev * 1.15);
        next
    };
    state.beta = calc(stats.bytes_stored, 0.004, state.beta);
}
```

### Operator ROI

For an operator providing role \(x\):

\[
\text{ROI}_x = \frac{\text{subsidy}_x \times \text{blocks\_per\_year}}{\text{stake}_x + \text{opex}_x}
\]

with yearly subsidy

\[
\text{subsidy}_x = \phi_x \cdot I_{\text{target}} \cdot S \cdot \frac{\text{stake\_share}_x}{\text{total\_effective\_stake}_x}.
\]

See [README](../README.md) for a high-level overview.
