# System-Wide Economic Changes

This living document chronicles every deliberate shift in The‑Block's protocol economics and system-wide design. Each section explains the historical context, the exact changes made in code and governance, the expected impact on operators and users, and the trade-offs considered. Future hard forks, reward schedule adjustments, or paradigm pivots must append an entry here so auditors can trace how the chain evolved.

## 2024 Credit Ledger Removal and CT Subsidy Transition

### Background: Legacy Three-Token Model
For the first development iterations, The‑Block experimented with a tri-token economy. Consumptive actions such as storing blobs or serving HTTP reads were paid with non-transferable *credits* that decayed over time, while CT and IT remained liquid utility tokens. Credits were earned via uptime, storage proofs, or domain budgets, and providers had to periodically swap or burn them to realize value. Documentation and code referenced a `read_reward_pool` that minted credits to reimburse gateways for free user reads.

### Rationale for the Switch
- **Operational Friction:** Operators juggled credit balances, decay curves, and swap mechanics. New node deployments stalled when credits ran out.
- **Regulatory Ambiguity:** A third instrument complicated token classifications and raised compliance review costs.
- **Illiquid Rewards:** Providers received a balance that could not be traded, hedged, or easily accounted for in fiat terms.
- **Simpler UX:** The project vision prioritizes "wallet opens, everything works". Requiring credits contradicted that mandate.

After multiple testnet cycles and stakeholder discussions, governance approved a migration to a pure CT/IT model where inflation-funded subsidies replace the credit ledger.

### Implementation Summary
- Deleted the entire `crates/credits` module, associated RPC endpoints, and `credits.db` persistence.
- Introduced global subsidy multipliers `beta`, `gamma`, `kappa`, and `lambda` for storage, read delivery, CPU, and bytes out. These values live in governance parameters and can be hot-tuned.
- Added a rent-escrow mechanism: every stored byte locks `rent_rate_ct_per_byte` CT, refunding 90 % on deletion or expiry while burning 10 % as wear-and-tear.
- Reworked coinbase generation so each block mints `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` alongside the decaying base reward.
- Redirected all former credit slashing paths to explicit CT burns, ensuring punitive actions reduce circulating supply.

Each change shipped behind feature flags during the transition period and was
accompanied by migration scripts (`scripts/zero_credits_db.sh` and new genesis
templates) so operators could replay devnet ledgers and verify that balances and
stake weights matched across the switch. Blocks produced before the transition
remain valid; the new fields simply appear as zero in historical headers.

### Impact on Operators
- Rewards arrive entirely in liquid CT, removing the need for off-chain credit swaps.
- Subsidy income now depends on verifiable work: bytes stored, bytes served with `ReadAck`, and measured compute. Stake bonds still back service roles, and slashing burns CT directly from provider balances.
- Monitoring requires watching the new counters: `subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, and rent-escrow gauges. Operators should also track `inflation.params` to observe multiplier retunes.

Operators are encouraged to archive `governance/history` to maintain a local
audit trail of multiplier votes and kill-switch activations. During the first
epoch after upgrade, double-check that telemetry exposes the new subsidy and
rent-escrow metrics; a missing gauge usually indicates lingering credit-era
config files or dashboard panels.

### Impact on Users
- Uploads, hosting, and dynamic requests no longer require preloading credits. Wallet creation suffices to start using storage or web hosting features.
- Reads remain free; the cost is socialized via block-level inflation rather than per-request fees. Users only see standard rate limits if they abuse the service.

Wallet interfaces now omit any credit balance widgets. When a user uploads a
file, the client transparently calculates the rent deposit, shows a preview of
the refundable amount, and submits the blob through the standard transaction
flow. Deleting the file later triggers the 90 % refund automatically, making the
economic lifecycle visible even to non-technical users.

### Governance and Telemetry
Governance manages the subsidy dial through `inflation.params`, which exposes the five parameters:
```
 beta_storage_sub_ct
 gamma_read_sub_ct
 kappa_cpu_sub_ct
 lambda_bytes_out_sub_ct
 rent_rate_ct_per_byte
```
An accompanying emergency knob `kill_switch_subsidy_reduction` can downscale all subsidies by a voted percentage. Every retune or kill‑switch activation must append an entry to `governance/history` and emits telemetry events for on-chain tracing.

The kill switch follows a 12‑hour timelock once activated, giving operators a
grace window to adjust expectations. Telemetry labels multiplier changes with
`reason="retune"` or `reason="kill_switch"` so dashboards can plot long-term
trends and correlate them with network incidents.

### Reward Formula Reference
The subsidy multipliers are recomputed each epoch using the canonical formula:
```
multiplier_x = (ϕ_x · I_target · S / 365) / (U_x / epoch_seconds)
```
where `S` is circulating CT supply, `I_target` is the annual inflation ceiling (currently 2 %), `ϕ_x` is the inflation share allocated to class `x`, and `U_x` is last epoch's utilization metric. Each multiplier is clamped to ±15 % of its prior value, doubling only if `U_x` was effectively zero to avoid divide-by-zero blow-ups. This dynamic retuning ensures inflation stays within bounds while rewards scale with real work.

### Pros and Cons
| Aspect | Credit Ledger | CT Subsidy Model |
|-------|---------------|------------------|
| Operator payouts | Non-transferable credits | Liquid CT every block |
| UX for new users | Requires earning or buying credits | Wallet works immediately |
| Governance surface | Credit mint/decay curves | Simple multiplier votes |
| Economic transparency | Harder to audit total issuance | Inflation capped ≤2 % with public multipliers |
| Regulatory posture | Extra instrument to explain | Two-token utility system |

### Migration Notes
Devnet operators should run `scripts/zero_credits_db.sh` to wipe obsolete ledgers and regenerate genesis files without `initial_credit_balances`. Faucet scripts now dispense CT. Operators must verify `inflation.params` after upgrade and ensure no `read_reward_pool` references persist in configs or dashboards.

### Future Entries
Subsequent economic shifts—such as changing the rent refund ratio, altering subsidy shares, or introducing new service roles—must document their motivation, implementation, and impact in a new dated section below. This file serves as the canonical audit log for all system-wide model changes.
