# State Storage

State persistence uses a RocksDB key-value store located under the path
supplied to the node via `--db-path` (default `~/.block/db`). Keys are stored
under a single column family and rely on RocksDB's write-ahead log for crash
recovery.

Compaction is triggered on every flush and may also be forced via the
`SimpleDb::compact` helper. Each compaction increments the
`storage_compaction_total` Prometheus counter when telemetry is enabled.

Schema migrations are tracked via the `__schema_version` key. Nodes upgrading
from earlier releases automatically bump the version to `4` through the helper
in `state::schema`, preserving existing data.

