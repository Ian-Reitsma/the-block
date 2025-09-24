## Schema v10 – Industrial subsidy fields and fee-split metadata
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Version 10 introduced per-block industrial subsidy fields (`storage_sub_it`, `read_sub_it`, `compute_sub_it`) and recorded fee-split metadata. These legacy columns remain zeroed in production now that CT is the sole transferable token, but they persist in the schema for backward compatibility and replaying historical snapshots.
Existing snapshots and chain databases are upgraded in-place by zero-filling
the new fields and bumping the `schema_version` to `10`.
