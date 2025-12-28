# Economic Control Laws - Operator Runbook

**Last Updated:** 2025-12-05
**Status:** Production-Ready
**Responsible Team:** Economics & Governance

---

## Overview

This runbook provides emergency procedures and operational guidance for The Block's four-layer economic control system. The control laws execute automatically at every epoch boundary (120 blocks, ~2 minutes) to maintain target inflation, reallocate subsidies, adjust market multipliers, and tune ad market splits and tariffs.

**Quick Reference:**
- Epoch interval: 120 blocks (~2 minutes)
- Control law execution: `node/src/lib.rs:4725-4778`
- Telemetry prefix: `economics_*`
- Dashboard: `docs/grafana_economics_dashboard.json`

---

## Table of Contents

1. [Normal Operations](#normal-operations)
2. [Emergency Procedures](#emergency-procedures)
3. [Monitoring & Alerts](#monitoring--alerts)
4. [Troubleshooting](#troubleshooting)
5. [Manual Interventions](#manual-interventions)
6. [Governance Parameter Tuning](#governance-parameter-tuning)

---

## Normal Operations

### Expected Behavior

**Layer 1: Inflation Control**
- Target: 5% annual inflation (500 bps)
- Convergence: Within ±1% (100 bps) after 30 epochs (~1 hour)
- Adjustment mechanism: Proportional feedback controller (k=0.10)
- Issuance bounds: 100M - 1B BLOCK per year

**Layer 2: Subsidy Allocation**
- Reallocation frequency: Every epoch
- Distress threshold: Margin < target or utilization deviation > 20%
- Drift rate: 1% per epoch toward optimal allocation
- Markets: Storage (15%), Compute (30%), Energy (20%), Ad (35%)

**Layer 3: Market Multipliers**
- Dual control: Utilization targeting + cost coverage
- Range: 0.10 - 10.0 per market
- Response speed: 5-10 epochs for 50% adjustment
- Ceiling alerts: Trigger if multiplier > 9.5

**Layer 4: Ad & Tariff**
- Ad splits drift: 1% per epoch toward targets
- Target splits: Platform 28%, User 22%, Publisher 50%
- Tariff drift: 5% per epoch toward target contribution
- Tariff bounds: 0-200 bps (0-2%)

### Health Indicators

**Green (Healthy):**
- Inflation error < 50 bps
- All multipliers in range [0.5, 5.0]
- Subsidy shares stable (σ < 100 bps over 1 hour)
- Treasury contribution within ±2% of target

**Yellow (Attention):**
- Inflation error 50-100 bps
- Any multiplier > 7.0 or < 0.2
- Subsidy oscillation (σ > 100 bps)
- Tariff at min/max bounds

**Red (Critical):**
- Inflation error > 200 bps for > 15 epochs
- Any multiplier at ceiling (10.0) or floor (0.1)
- Negative provider margins for > 30 epochs
- Treasury contribution < 5% or > 20%

---

## Emergency Procedures

### 1. Runaway Inflation (Realized > Target + 200 bps)

**Symptoms:**
```
economics_inflation_error_bps > 200 for 15+ epochs
economics_annual_issuance_block increasing rapidly
```

**Immediate Actions:**
1. Check circulating supply calculation:
   ```bash
   contract-cli explorer state --field emission_consumer
   ```
2. Verify epoch boundary execution:
   ```bash
   grep "Execute economic control laws" <node-logs> | tail -20
   ```
3. Check for unexpected BLOCK minting:
   ```bash
   contract-cli explorer blocks --tail 100 | grep coinbase
   ```

**Root Causes & Fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| Supply calculation error | `emission_consumer` drifting | File governance proposal to reset circulating supply snapshot |
| Controller gain too low | Inflation error persistent | Increase `inflation_controller_gain` via governance (default: 100 millis → try 150) |
| Max issuance hit | `economics_annual_issuance_block` == `max_annual_issuance_block` | Raise `max_annual_issuance_block` via governance |
| Non-epoch minting | Extra coinbase payouts | Emergency: Hard fork to fix minting bug |

**Recovery Time:** 30-60 epochs (~1-2 hours) with controller gain adjustment

---

### 2. Market Multiplier at Ceiling (>= 9.5)

**Symptoms:**
```
economics_market_multiplier{market="energy"} >= 9.5
```

**Meaning:**
- Market is in severe distress
- Providers unprofitable even with 10x subsidies
- Utilization critically low or costs too high

**Immediate Actions:**
1. Identify distressed market:
   ```bash
   curl http://localhost:9090/api/v1/query?query='economics_market_multiplier' | jq
   ```
2. Check utilization and margins:
   ```bash
   curl http://localhost:9090/api/v1/query?query='economics_market_utilization{market="energy"}' | jq
   curl http://localhost:9090/api/v1/query?query='economics_provider_margin{market="energy"}' | jq
   ```
3. Review recent parameter changes:
   ```bash
   contract-cli gov proposals --status executed | grep -E "energy|multiplier"
   ```

**Root Causes & Fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| Target utilization too high | `energy_util_target_bps` > actual capacity | Lower via governance (e.g., 8000 → 6000) |
| Cost target too low | Actual costs >> `energy_margin_target_bps` | Raise margin target (e.g., 1000 → 1500) |
| No providers | `provider_count{market="energy"}` == 0 | Bootstrap incentives or manual subsidy increase |
| Ceiling too low | Legitimate need for 10x+ subsidy | Raise `energy_multiplier_ceiling` (e.g., 10.0 → 15.0) |

**Recovery Time:** 20-30 epochs after parameter adjustment

---

### 3. Subsidy Oscillation (Rapid Reallocation)

**Symptoms:**
```
stddev(economics_subsidy_share_bps{market="storage"}[1h]) > 200
```

**Meaning:**
- Allocator unstable
- Temperature parameter too high
- Multiple markets in simultaneous distress

**Immediate Actions:**
1. Visualize subsidy trends:
   ```bash
   # Use Grafana dashboard "Layer 2: Subsidy Allocation Shares"
   ```
2. Check distress scores:
   ```bash
   grep "distress" <node-logs> | tail -50
   ```
3. Verify drift rate:
   ```bash
   contract-cli gov params get subsidy_allocator_drift_rate
   ```

**Root Causes & Fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| Temperature too high | Softmax over-reacting to small distress changes | Lower `subsidy_allocator_temperature` (10000 → 5000) |
| Drift rate too fast | Allocations swing >10% per epoch | Lower `subsidy_allocator_drift_rate` (10 → 5) |
| Alpha too high | Over-weight utilization vs. margins | Balance `alpha` and `beta` (500/500 → 400/600) |
| Multiple distress | All markets struggling | Emergency subsidy injection via treasury |

**Recovery Time:** 10-20 epochs after temperature/drift adjustment

---

### 4. Tariff Stuck at Bounds

**Symptoms:**
```
economics_tariff_bps == 200  # Max
economics_treasury_contribution_bps < 500  # Still below target
```

**Meaning:**
- Non-KYC volume insufficient to meet treasury target
- Tariff maxed out but still not generating enough revenue

**Immediate Actions:**
1. Check non-KYC volume:
   ```bash
   curl http://localhost:9090/api/v1/query?query='economics_tariff_bps' | jq
   ```
2. Review treasury inflows:
   ```bash
   contract-cli gov treasury balance
   ```
3. Calculate required tariff:
   ```bash
   # Required: (target_bps / 10000) * treasury_inflow / non_kyc_volume
   # If result > max_tariff_bps, tariff is insufficient
   ```

**Root Causes & Fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| Tariff ceiling too low | Required tariff > `tariff_max_bps` | Raise ceiling (200 → 300 via governance) |
| Non-KYC volume too low | Most users are KYC verified | Adjust `public_revenue_target_bps` lower (1000 → 500) |
| Treasury target too high | Target contribution unrealistic | Lower target or diversify revenue sources |
| Low overall volume | Network underutilized | Marketing/adoption efforts, not control law fix |

**Recovery Time:** Immediate after bound adjustment

---

### 5. Negative Provider Margins

**Symptoms:**
```
economics_provider_margin{market="compute"} < 0 for 30+ epochs
```

**Meaning:**
- Providers losing money
- Costs exceed payouts even with subsidies
- Market may see provider exodus

**Immediate Actions:**
1. Identify affected markets:
   ```bash
   curl http://localhost:9090/api/v1/query?query='economics_provider_margin < 0' | jq
   ```
2. Check subsidy multipliers:
   ```bash
   curl http://localhost:9090/api/v1/query?query='economics_market_multiplier' | jq
   ```
3. Review payout calculations:
   ```bash
   contract-cli explorer settlements --market compute --tail 100
   ```

**Root Causes & Fixes:**

| Cause | Diagnosis | Fix |
|-------|-----------|-----|
| Margin target too low | `compute_margin_target_bps` < actual costs | Raise target (500 → 1000) |
| Responsiveness too low | Multiplier not responding fast enough | Increase `cost_responsiveness` (500 → 800) |
| Base subsidy too low | Even 10x multiplier insufficient | Increase base subsidy amount via governance |
| Market manipulation | Artificially low prices | Investigate + potentially ban bad actors |

**Recovery Time:** 15-25 epochs after margin target adjustment

---

## Monitoring & Alerts

### Grafana Dashboard Setup

1. Import dashboard template:
   ```bash
   curl -X POST http://grafana:3000/api/dashboards/db \
     -H "Content-Type: application/json" \
     -d @docs/grafana_economics_dashboard.json
   ```

2. Configure alerts (already in template):
   - **Inflation Divergence:** Error > 100 bps for 5 minutes
   - **Multiplier Ceiling:** Any multiplier > 9.5 for 5 minutes
   - **Negative Margin:** Any market margin < 0 for 5 minutes

### Prometheus Queries

**Inflation Health:**
```promql
abs(economics_inflation_error_bps) / 100  # % deviation
rate(economics_annual_issuance_block[1h])    # Issuance trend
```

**Subsidy Stability:**
```promql
stddev_over_time(economics_subsidy_share_bps[1h])  # Volatility
changes(economics_subsidy_share_bps{market="storage"}[1h])  # Churn
```

**Market Health:**
```promql
economics_market_multiplier > 7  # Distress threshold
min(economics_provider_margin) by (market)  # Worst margin
```

**Treasury Contribution:**
```promql
economics_treasury_contribution_bps / 100  # Actual %
abs(economics_treasury_contribution_bps - 1000) / 100  # Target deviation
```

### Deterministic Metrics Checks

- `tb-cli governor status --rpc <endpoint>` now prints both the persisted `economics_sample` snapshot and a `telemetry gauges (ppm)` section derived from `economics_prev_market_metrics`. Use that output to confirm the controller’s persisted view matches the Prometheus gauges `economics_prev_market_metrics_{utilization,provider_margin}_ppm` before trusting any gate transition.
- Cross-check the intent logs (`tb-cli governor intents --gate economics`) against those ppm snapshots whenever you investigate flapping or unexpected exits.

---

## Troubleshooting

### Control Laws Not Executing

**Symptoms:**
- `economics_annual_issuance_block` not updating
- Telemetry stale for multiple epochs

**Diagnosis:**
```bash
# Check if node is mining
contract-cli explorer blocks --tail 10

# Check epoch boundary hits
grep "Epoch boundary" <node-logs> | tail -20

# Verify control law execution
grep "Execute economic control laws" <node-logs> | tail -10
```

**Fixes:**
- If mining stopped: Restart node or check difficulty
- If epoch not triggering: Check block height modulo 120
- If execution failing: Check logs for panics in `execute_epoch_economics`

### Telemetry Not Updating

**Symptoms:**
- Grafana shows no data
- Prometheus scrape failures

**Diagnosis:**
```bash
# Check telemetry feature enabled
cargo build -p the_block --features telemetry

# Verify metrics endpoint
curl http://localhost:9091/metrics | grep economics_

# Check Prometheus config
grep economics /etc/prometheus/prometheus.yml
```

**Fixes:**
- Rebuild with `--features telemetry`
- Add metrics port to Prometheus scrape config
- Check firewall rules for port 9091

### Parameter Changes Not Taking Effect

**Symptoms:**
- Governance proposal executed
- Control law behavior unchanged

**Diagnosis:**
```bash
# Verify proposal execution
contract-cli gov proposals --id <proposal-id> --status

# Check current parameters
contract-cli gov params get inflation_target_bps

# Verify node has latest params
grep "from_governance_params" <node-logs> | tail -1
```

**Fixes:**
- Wait for next epoch boundary (parameters update per-epoch)
- Verify proposal activated (ACTIVATION_DELAY = 2 epochs)
- Check node synced to latest block

---

## Manual Interventions

### Emergency Parameter Override

**When to Use:**
- Critical inflation divergence
- Market collapse imminent
- Control law bug discovered

**Procedure:**
1. **Draft emergency proposal:**
   ```bash
   contract-cli gov propose \
     --title "EMERGENCY: Fix inflation controller gain" \
     --body "Inflation error exceeding 500 bps. Doubling controller gain." \
     --param inflation_controller_gain=200
   ```

2. **Fast-track voting:**
   - Coordinate with validators for rapid approval
   - Aim for 2-epoch turnaround (QUORUM = 66%)

3. **Monitor post-activation:**
   ```bash
   watch -n 10 'curl -s http://localhost:9090/api/v1/query?query=economics_inflation_error_bps | jq'
   ```

### Temporary Subsidy Injection

**When to Use:**
- Market multiplier at ceiling
- Providers exiting en masse
- Need time for governance to act

**Procedure:**
1. **Calculate required injection:**
   ```
   needed = (target_payout - current_payout) * provider_count * epochs
   ```

2. **Treasury disbursement:**
   ```bash
   contract-cli gov disburse propose \
     --recipient <market_subsidy_pool> \
     --amount <needed> \
     --justification "Emergency: Compute market collapse"
   ```

3. **Manual market maker:**
   - Temporarily subsidize specific providers
   - Bridge gap until control law stabilizes

---

## Governance Parameter Tuning

### Safe Tuning Ranges

| Parameter | Default | Safe Range | Danger Zone | Effect |
|-----------|---------|------------|-------------|--------|
| `inflation_target_bps` | 500 (5%) | 300-800 | <100 or >1500 | Sets target inflation rate |
| `inflation_controller_gain` | 100 (0.10) | 50-200 | <20 or >500 | Response speed |
| `subsidy_allocator_temperature` | 10000 (10.0) | 5000-15000 | <1000 or >50000 | Softmax sharpness |
| `subsidy_allocator_drift_rate` | 10 (0.01) | 5-20 | <1 or >100 | Reallocation speed |
| `*_multiplier_ceiling` | 10000 (10.0) | 5000-20000 | <3000 or >50000 | Max subsidy boost |
| `tariff_max_bps` | 200 (2%) | 100-500 | <50 or >1000 | Max non-KYC fee |

### Tuning Workflow

1. **Identify issue** using monitoring dashboard
2. **Simulate fix** using integration tests:
   ```bash
   # Edit node/tests/economics_integration.rs with new params
   cargo test -p the_block --test economics_integration
   ```
3. **Draft proposal** with justification citing metrics
4. **Small adjustments** (10-20% changes, not 2-3x)
5. **Monitor convergence** over 50+ epochs

### Parameter Interdependencies

**Inflation Controller:**
- `controller_gain` ↑ → faster convergence, more oscillation
- `min/max_annual_issuance_block` → hard bounds, prevents runaway

**Subsidy Allocator:**
- `temperature` ↑ → sharper allocation, faster swings
- `drift_rate` ↑ → faster adjustment, less stability
- `alpha` vs `beta` → balance utilization vs margin weighting

**Market Multipliers:**
- `util_responsiveness` ↑ → react faster to demand
- `cost_responsiveness` ↑ → react faster to profitability
- `ceiling` ↑ → allow more extreme subsidies

**Ad & Tariff:**
- `ad_drift_rate` ↑ → splits converge faster
- `tariff_drift_rate` ↑ → tariff adjusts faster
- `public_revenue_target_bps` ↑ → higher tariff needed

---

## Appendix: Key File Locations

| Component | File Path | Line Numbers |
|-----------|-----------|--------------|
| Control law execution | `node/src/lib.rs` | 4725-4778 |
| Inflation controller | `node/src/economics/inflation_controller.rs` | 1-150 |
| Subsidy allocator | `node/src/economics/subsidy_allocator.rs` | 1-300 |
| Market multipliers | `node/src/economics/market_multiplier.rs` | 1-200 |
| Ad & tariff | `node/src/economics/ad_market_controller.rs` | 1-360 |
| Telemetry updates | `node/src/telemetry.rs` | (see `update_economics_*` functions) |
| Governance params | `governance/src/params.rs` | 392-700 |
| Integration tests | `node/tests/economics_integration.rs` | 1-300 |
| Grafana dashboard | `docs/grafana_economics_dashboard.json` | 1-292 |

---

## Contact & Escalation

**For emergencies:**
1. Check this runbook first
2. Review Grafana dashboard
3. Check recent governance proposals
4. Escalate to Economics Team lead
5. If critical: Emergency hard fork protocol

**Non-emergency support:**
- Create issue in project repo
- Tag `economics` label
- Reference metrics/logs

---

**Document Version:** 1.0
**Next Review:** When control laws are modified or new layers added
