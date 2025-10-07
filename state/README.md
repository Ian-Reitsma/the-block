# State Storage
> **Review (2025-09-25):** Synced State Storage guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

State persistence uses a RocksDB key-value store located under the path
supplied to the node via `--db-path` (default `~/.block/db`). Keys are stored
under a single column family and rely on RocksDB's write-ahead log for crash
recovery. When additional column families are created (for shard-specific
state, receipts, etc.), `SimpleDb` caches the underlying RocksDB handles as
`ColumnFamily` values so callers pass lightweight references to
`get_cf`/`put_cf`/`delete_cf`. `ColumnFamily` implements `Send`/`Sync`, which
lets the wrapper remain thread-safe without leaking `BoundColumnFamily` pointers
that fail PyO3's thread-checks.

The periodic storage repair task (`storage::repair::spawn`) now runs inside
Tokio's blocking pool and opens `SimpleDb` on that worker thread, avoiding the
need to hold a database handle across `.await` points. A test hook exercises the
loop to ensure it continues to fire on schedule without leaking the background
worker between test runs.

Crash recovery is verified by tests under `state/tests/crash_recovery.rs`,
which reopen the database after an abrupt drop to ensure committed writes are
preserved.

Compaction is triggered on every flush and may also be forced via the
`SimpleDb::compact` helper. Each compaction increments the
`storage_compaction_total` Prometheus counter when telemetry is enabled.

Schema migrations are tracked via the `__schema_version` key. Nodes upgrading
from earlier releases automatically bump the version to `4` through the helper
in `state::schema`, preserving existing data.

The `sim` crate can replay scenarios against RocksDB by selecting the
`Backend::RocksDb` option and setting `SIM_DB_PATH`. Each simulation step is
serialized with a compact first-party binary format and stored keyed by step number for later analysis.

