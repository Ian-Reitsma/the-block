# Threat Model Overview
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This folder outlines adversarial surfaces and the counter-measures baked into the CT-only design.

## Cross-cutting Controls

- **Bonded service roles** – gateways, storage, and exec nodes lock CT and are slashable on fraud.
- **Inflation governors** – multipliers \(\beta,\gamma,\kappa,\lambda\) auto-retune each epoch and fall back 5 % when annualised inflation exceeds 2 %.
- **Kill switch** – `kill_switch_subsidy_reduction` can globally scale down all multipliers after a 12 h timelock.
- **Salted IP hashing** – telemetry scrubs IPs via `SHA256(epoch‖ip)` before export.

The subsidy multipliers follow the canonical formula:

\[
\text{multiplier}_x = \frac{\phi_x \cdot I_{\text{target}} \cdot S / 365}{U_x / \text{epoch\_seconds}}
\]

Clamped to ±15 % of the prior value and doubled when utilisation is ≈0.

See [../economics.md](../economics.md#epoch-retuning-formula) for derivation and ROI guidance.