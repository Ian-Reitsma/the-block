# v8 – Bridge Header Store

## Summary
- add `verified_headers` set to bridge state to prevent replay of external deposits
- persist each verified header under `state/bridge_headers/<hash>`

## Migration
Older snapshots lacking the new field continue to deserialize by default. No manual action is required; the `Bridge` struct uses `#[serde(default)]` for `verified_headers` and will initialise an empty set when loading pre-v8 data.
