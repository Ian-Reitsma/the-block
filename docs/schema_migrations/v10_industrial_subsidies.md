## Schema v10 – Industrial subsidy fields and fee-split metadata

Version 10 introduces per-block industrial subsidy fields (`storage_sub_it`,
`read_sub_it`, `compute_sub_it`) and records mixed CT/IT fee information.
Existing snapshots and chain databases are upgraded in-place by zero-filling
the new fields and bumping the `schema_version` to `10`.

