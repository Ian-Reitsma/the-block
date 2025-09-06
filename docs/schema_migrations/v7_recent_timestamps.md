# Schema v7: recent timestamp history

Version 7 introduces a `recent_timestamps` vector in `ChainDisk` so nodes can
retarget proof-of-work difficulty using a sliding window of block times.

## Migration

* Previous version: 6
* On upgrade, initialize `recent_timestamps` as an empty list and set
  `schema_version` to 7.
* Older binaries (schema < 7) refuse to open databases marked with this version.
