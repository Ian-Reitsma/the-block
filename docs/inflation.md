## Inflation and Industrial Demand Metrics
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The `inflation.params` RPC now returns the current subsidy multipliers along
with industrial backlog and utilisation metrics. These metrics drive the
industrial subsidy retuning logic and allow operators to track compute demand.

Example response:

```json
{
  "beta_storage_sub_ct": 50,
  "gamma_read_sub_ct": 20,
  "kappa_cpu_sub_ct": 10,
  "lambda_bytes_out_sub_ct": 5,
  "industrial_multiplier": 100,
  "industrial_backlog": 0,
  "industrial_utilization": 0
}
```

## Industrial Subsidy Retuning

The governor maintains a separate Kalman state vector
`(industrial_beta, industrial_gamma, industrial_kappa, industrial_lambda)`
covering storage, read, compute, and bytes‑out multipliers. Each epoch the
filter ingests `industrial_backlog` and `industrial_utilization` metrics and
emits smoothed multipliers for the next period.

`Block::industrial_subsidies()` exposes per‑block IT payouts:

```rust
let (s, r, c) = block.industrial_subsidies();
```

These fields (`storage_sub_it`, `read_sub_it`, `compute_sub_it`) appear in block
RPC responses:

```bash
curl -s localhost:26658/chain.block?height=1 | \
  jq '.block | {storage_sub_it,read_sub_it,compute_sub_it}'
# {"storage_sub_it":0,"read_sub_it":0,"compute_sub_it":0}
```

Ledger changes introducing the industrial subsidy fields are described in
[`docs/schema_migrations/v10_industrial_subsidies.md`](schema_migrations/v10_industrial_subsidies.md).