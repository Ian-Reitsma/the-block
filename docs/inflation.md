## Inflation and Industrial Demand Metrics

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

