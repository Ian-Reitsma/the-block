# ECONOMICS — Fee Routing Glossary and Invariants
> **Review (2025-09-25):** Synced ECONOMICS — Fee Routing Glossary and Invariants guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

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

## Logistic Miner Reward Calibration

Base block rewards shrink as the effective miner count exceeds the target
`miner_reward_logistic_target`. The curve slope is governed by
`logistic_slope_milli` (×1000). Each epoch the node evaluates

\[
f(N) = \frac{1}{1 + e^{\xi (N - N^*)}},\qquad \xi = \text{logistic\_slope\_milli}/1000
\]

with $N$ the Rényi-effective miner count and $N^*$ the target. Telemetry
exports `active_miners`, `base_reward_ct`, and the counter
`miner_reward_recalc_total` to expose hysteresis transitions.

## del‑Pino Logarithmic AMM Invariant

To keep DEX pricing path‑independent under volatility clustering we adopt the
del‑Pino curve:

\[
x \ln x + y \ln y = k
\]

Swaps solve for the post‑trade reserve `y'` given input `Δx` such that the
invariant holds; the output is `Δy = y - y'`. See `sim/src/dex.rs` for the
Newton solver and property tests.

## Inflation Cap Proof‑of‑Bound

Each week validators publish a Merkle root of tuples
\((week, S_{start}, S_{end}, \rho_{calc}, \sigma_S)\), signed by ≥⅔ stake.
Light clients verify the published root to ensure
\(\rho_{calc} ≤ 0.02\) without replaying the chain. `node/src/governance/
inflation_cap.rs` implements the root computation.

## Compute-Backed Money

Compute-backed tokens (CBTs) redeem for compute units at a protocol-defined
curve. A linear redeem curve priced off the marketplace median ensures large
burns incur a premium, slowing depletion of the fee-funded backstop. Fees from
compute jobs replenish the backstop reserve.

The `ComputeToken` prototype models this flow. Tokens represent compute units
and redeem against a `RedeemCurve` while debiting a shared `Backstop` reserve.
See `node/tests/compute_cbt.rs` for an instant-app style settlement example.
## Epoch Retuning Formula

The chain mints storage, read, and compute subsidies in CT via block fields `storage_sub_ct`, `read_sub_ct`, and `compute_sub_ct`.
At each epoch the multipliers
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

To damp bursty utilization, the input `U_x` is smoothed with an adaptive
Golden‑Section window. Starting from a single epoch (`d_0 = 1`), the window
grows by the golden ratio (`d_{k+1} = ⌈ϕ·d_k⌉`) until the variance over the
window satisfies `Var(U_{t-d_k:t}) ≤ (0.1·U_t)^2`. The mean over the minimal
window is then used in the formula above, providing just enough memory to
stabilise the controller without unnecessary lag.

For example, assume the chain stored 12 GB of new blobs and served 80 GB of
reads during the last epoch while circulating supply sat at 900 million CT. With
\(\phi_{storage}=0.004\) and an epoch length of 6 000 seconds, the storage
multiplier computes as:

\[
\beta = \frac{0.004 \times 0.02 \times 900_000_000 / 365}{12\,000\,000 / 6_000} \approx 0.009 \text{ µCT/byte}.
\]

If the previous \(\beta\) was 0.008 µCT/B, the clamp allows at most 0.0092
µCT/B, so the multiplier settles at 0.0092. The same procedure applies to the
read and compute multipliers, providing predictable, gradual reward scaling
even under sudden utilization swings.

### Read subsidy distribution & advertising offsets

Prior to 2025-10 the entire `read_sub_ct` allocation flowed into the miner
coinbase. The protocol now tracks per-role byte totals during the epoch and
splits the minted CT across governance-controlled buckets when the block
finalizes:

- `read_sub_viewer_ct` – CT credited directly to the wallet that requested the
  content.
- `read_sub_host_ct` – CT credited to the domain’s hosting stake account.
- `read_sub_hardware_ct` – CT for the physical provider that served the bytes.
- `read_sub_verifier_ct` – CT rewarding the verification network.
- `read_sub_liquidity_ct` – CT routed to the liquidity pool to back marketplace
  swaps.
- `read_sub_ct - Σ(read_sub_*_ct)` – residual CT that continues to flow to the
  miner once all roles are satisfied.

Governance parameters (`read_subsidy_viewer_percent`, `read_subsidy_host_percent`,
`read_subsidy_hardware_percent`, `read_subsidy_verifier_percent`, and
`read_subsidy_liquidity_percent`) express the target percentages for each role;
any shortfall due to missing addresses automatically rolls into the liquidity
share so minted CT remains conserved. Explorers and RPC snapshots expose every
per-role field so wallets and settlement tooling can display the credited
amounts without replaying acknowledgement batches.

Advertising campaigns further augment the read payouts. When the gateway matches
an impression to a campaign, the committed CT is split using the same
distribution policy and materializes in the block as `ad_viewer_ct`,
`ad_host_ct`, `ad_hardware_ct`, `ad_verifier_ct`, `ad_liquidity_ct`, and
`ad_miner_ct`. These fields settle reserved campaign budget against the
acknowledged reads without diluting the inflation schedule.

### Advertising price discovery and dual-token settlement

Impression pricing is now quoted in USD and adapts to cohort utilisation. Each
cohort maintains a posted price `p_{MiB,c}` that evolves according to a
log-domain PI controller with exponential forgetting:

\[
\ln p_{MiB,c}(t + \Delta) = \ln p_{MiB,c}(t) + \eta_P\,[\tilde{U}_c - 1]
  + \eta_I\,I_c(t),
\]

with the exponentially-weighted integral term

\[
I_c(t) = e^{-\rho \Delta} I_c(t-\Delta) + [\tilde{U}_c - 1],
\]

and `\tilde{U}_c = min(1, demand_c / (supply_c * p_{MiB,c})) / U_c^*`. The
controller clamps `|\eta_P| \le 0.25` and `\eta_I \le 0.05 |\eta_P|` to keep
updates well-damped even in thin cohorts while the forgetting factor `\rho`
prevents integral windup during demand shocks. When demand exceeds supply the
price nudges upward; slack capacity drives the rate down without imposing
artificial caps. The in-memory and sled market implementations both record the
price and demand deltas per reservation so historical utilisation feeds future
updates.

Campaign budgets remain denominated in USD micros. When an impression commits,
the marketplace records both the USD total and the oracle snapshot inside the
`SettlementBreakdown` structure:

- `total_usd_micros` captures the billed amount before rounding losses.
- `price_per_mib_usd_micros` records the marginal USD price applied to the
  impression payload, while `clearing_price_usd_micros` stores the auction
  clearing price that determined the payment.
- `ct_price_usd_micros` and `it_price_usd_micros` store the CT/IT oracle
  prices that were applied during conversion.
- `delivery_channel` enumerates whether the impression was delivered over HTTP
  or mesh (`DeliveryChannel::Mesh`), and `mesh_payload`/`mesh_payload_digest`
  surface the staged payload bytes and their BLAKE3 digest when RangeBoost mesh
  delivery was used.
- `viewer_ct`, `host_ct`, `hardware_ct`, `verifier_ct`, `liquidity_ct`, and
  `miner_ct` represent the CT settlement that continues to feed the on-chain
  ledger.
- `host_it`, `hardware_it`, `verifier_it`, `liquidity_it`, and `miner_it`
  expose the mirrored IT token quantities that now land in the ledger, explorer,
  and CLI pipelines alongside the CT totals.
- `unsettled_usd_micros` records the residual USD value that could not be
  expressed as whole CT/IT tokens.

Governance still controls the share allocated to each role through
`DistributionPolicy`. The `liquidity_split_ct_ppm` knob determines what
percentage of the liquidity allocation settles in CT versus IT. Ledgers now
record both token flows plus the oracle snapshot per block, and downstream
analytics (explorer, CLI, dashboards, and CI artefacts) read the CT/IT split and
prices directly from those records so operators can audit both currencies in
parallel without recomputing conversions.

The marketplace applies the split before minting tokens: the CT conversion uses
only the `liquidity_split_ct_ppm` share, while the remaining USD routes to the
IT conversion path. This avoids double counting liquidity budgets and ensures CT
totals stay consistent with the legacy ledger while IT payouts surface for
governance and observability. Debug assertions guard the conversion helper so the
minted tokens (plus their rounding remainder) recombine to the original USD
allocation, and a rounding regression covers uneven oracle prices to prove the
split holds even when liquidity contributes only partial token units.

```rust
fn retune_multipliers(state: &ChainState, stats: &UtilStats) {
    let s = state.ct_supply();
    let epoch_secs = stats.epoch_secs as f64;
    let target = 0.02;
    let yr_secs = 31_536_000.0;
    // Fibonacci-tempered smoothing with Hampel outlier rejection
    let eta = state.params.util_var_threshold as f64 / 1000.0;
    let base =
        (state.params.fib_window_base_secs as f64 / epoch_secs).ceil() as usize;
    let smooth = |hist: &[f64], current: f64| {
        const PHI: f64 = 1.618_033_988_749_894_8; // golden ratio
        if hist.is_empty() {
            return current;
        }
        let mut d = base.max(1);
        loop {
            let start = hist.len().saturating_sub(d);
            let slice = &hist[start..];
            let mut sorted = slice.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = sorted[sorted.len() / 2];
            let mut devs: Vec<f64> = slice.iter().map(|v| (v - median).abs()).collect();
            devs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mad = devs[devs.len() / 2].max(1e-9);
            let thresh = 3.0 * mad;
            let filtered: Vec<f64> = slice
                .iter()
                .cloned()
                .filter(|v| (v - median).abs() <= thresh)
                .collect();
            let mean = filtered.iter().sum::<f64>() / filtered.len() as f64;
            let var = if filtered.len() > 1 {
                filtered
                    .iter()
                    .map(|v| (v - mean).powi(2))
                    .sum::<f64>()
                    / filtered.len() as f64
            } else {
                0.0
            };
            if var <= eta * eta * current * current || d >= hist.len() {
                return mean;
            }
            d = (PHI * d as f64).ceil() as usize;
        }
    };
    let u = smooth(&hist.bytes_stored, stats.bytes_stored);
    let mut next = if u < 1.0 {
        state.beta * 2.0
    } else {
        let yearly = u * (yr_secs / epoch_secs);
        (0.004 * target * s) / yearly
    };
    next = next.clamp(state.beta * 0.85, state.beta * 1.15);
    state.beta = next;
}
```

Encrypted utilisation submissions are supported. Validators can broadcast an
`EncryptedUtilization` blob—bincode statistics XOR-ed with a shared key—and
the chain will decrypt and pass the result into
`retune_multipliers_encrypted`, preserving the entropy bound on raw
utilisation while still retuning multipliers.

## Base Reward & Miner Entropy

The decaying base reward adapts to miner concentration using φ‑entropy.
Let `h_i` be the share rate (blocks per second) for miner `i` over the last
120 blocks and `p_i = h_i / Σ_j h_j`. Define `H_φ = -ln Σ p_i^2` (φ = 2)
and the effective miner count `N_eff = e^{H_φ}`. The base reward is scaled by
`1/(1 + exp[ξ (N_eff - N*)])` where `N*` is the governance target and
`ξ = ln 99 / (0.1 N*)`.

### Operator ROI

For an operator providing role \(x\):

\[ 
\text{ROI}_x = \frac{\text{subsidy}_x \times \text{blocks\_per\_year}}{\text{stake}_x + \text{opex}_x}
\]

with yearly subsidy

\[
\text{subsidy}_x = \phi_x \cdot I_{\text{target}} \cdot S \cdot \frac{\text{stake\_share}_x}{\text{total\_effective\_stake}_x}.
\]

As an example, a gateway that bonds 10 000 CT when the total bonded gateway
stake is 200 000 CT controls a 5 % stake share. If governance allocates
\(\phi_{read} = 0.0025\) of the annual inflation budget to read delivery and
the circulating supply is 900 million CT, the gateway's expected yearly subsidy
is:

\[
\text{subsidy}_{read} = 0.0025 \times 0.02 \times 900_000_000 \times 0.05 \approx 2_250 \text{ CT/year}.
\]

Dividing this by the bonded stake and the operator's annual operating expenses
produces an estimated ROI, enabling hardware planning and break-even analyses.

See [README](../README.md) for a high-level overview.
