# Log Correlation and Search
> **Review (2025-09-25):** Synced Log Correlation and Search guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Structured logs include a per-request `correlation_id` field that links
individual log entries with telemetry metrics and external tooling.

Logs emitted as JSON can be indexed with the `log_indexer` utility:

```
$ cargo run --release --manifest-path tools/log_indexer_cli/Cargo.toml -- index <logfile> <log_store_dir>
$ cargo run --release --manifest-path tools/log_indexer_cli/Cargo.toml -- index <logfile> <log_store_dir> --passphrase secret
```

The indexer now persists each entry's timestamp, level, message and
correlation identifier inside a first-party key-value store backed by
the in-house storage engine while recording the last file offset it
processed. Subsequent runs resume automatically from the previous offset
and every insert increments both the `log_entries_indexed_total`
counter and the correlation-tagged
`log_correlation_index_total{correlation_id="…"}` metric for
observability. If you are upgrading from a legacy SQLite `.db` file,
pass the existing path to the new indexer and enable the
`sqlite-migration` feature (`cargo run --features sqlite-migration …`) to
perform an in-place import before switching back to the default build.

> **Serialization migration.** While the logging stack is still wired to the
> legacy JSON/TOML tooling, the new `foundation_serialization` crate already
> ships stub traits and helpers for JSON/CBOR/binary formats. Expect RPC and CLI
> entry points to start depending on those traits so we can excise `serde` and
> friends without keeping their derive macros in the hot path. Until the
> implementation lands the helpers intentionally return `unimplemented` errors
> to keep accidental linkage with the third-party crates impossible.

Once indexed, the CLI can query for specific correlation IDs, peer
identifiers, transaction hashes or block numbers via:

```
$ contract logs search --db log_store_dir --correlation <id>
$ contract logs search --db log_store_dir --peer peer-42 --since 1700000000 --until 1700003600
```

The metrics aggregator ingests the same `correlation_id` labels from
Prometheus payloads and caches the most recent correlations per metric.
Each ingestion updates the `log_correlation_index_total` counter and,
when lookups fail to find matching rows, the
`log_correlation_fail_total` counter increments for alerting. The
aggregator exposes the cached mapping at:

```
GET http://<aggregator>/correlations/<metric>
```

where `<metric>` might be `quic_handshake_fail_total`. When
`quic_handshake_fail_total` increases for a peer, the aggregator will
query the node's `/logs/search` endpoint (configured via
`TB_LOG_API_URL` and `TB_LOG_DB_PATH`) and persist a JSON dump under
`$TB_LOG_DUMP_DIR` (default `log_dumps/`). Operators receive a log line
with the dump path and the associated correlation ID.

The CLI provides a convenience wrapper that stitches these pieces
together:

```
$ contract logs correlate-metric --metric quic_handshake_fail_total \
      --aggregator http://localhost:9000 --rows 25 --max-correlations 3
```

The command pulls recent correlations from the aggregator, prompts for a
passphrase if required, and prints the matching log excerpts. If no
database path is provided the CLI falls back to `TB_LOG_DB_PATH`.

If `--passphrase` is omitted the command prompts securely. Operators can
rotate encrypted payloads in-place using:

```
$ contract logs rotate-key --db log_store_dir --old-passphrase old --new-passphrase new
```

Encrypted messages are decrypted on the fly when the same passphrase is
supplied. Messages without a passphrase are displayed as `<encrypted>`
so operators can still correlate telemetry without exposing payloads.

### REST search API

Nodes expose a lightweight REST endpoint that mirrors the CLI filters:

```
GET /logs/search?level=ERROR&since=1700000000&until=1700003600&limit=100
GET /logs/search?correlation=beta&passphrase=secret
```

If `db` is omitted the handler uses the `TB_LOG_DB_PATH` environment
variable. The same filters as the CLI are available: `peer`, `tx`,
`block`, `correlation`, `level`, `since`, `until`, `after-id`, `limit`
and `passphrase`.

Live tailing is available over WebSocket via `/logs/tail` with the same
query parameters plus `interval_ms` to control polling cadence. Each
frame carries a JSON array of `LogEntry` records for easy streaming into
dashboards.

### Load testing helper

`scripts/log_indexer_load.sh` stress-tests the indexer against a million
synthesised log lines and performs a sample filtered query:

```
$ ./scripts/log_indexer_load.sh          # default 1,000,000 rows
$ ./scripts/log_indexer_load.sh 250000   # alternate volume
```
