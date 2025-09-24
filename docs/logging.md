# Log Correlation and Search
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Structured logs include a per-request `correlation_id` field that links
individual log entries with telemetry metrics and external tooling.

Logs emitted as JSON can be indexed with the `log_indexer` utility:

```
$ cargo run --release --manifest-path tools/log_indexer_cli/Cargo.toml -- index <logfile> <sqlite.db>
$ cargo run --release --manifest-path tools/log_indexer_cli/Cargo.toml -- index <logfile> <sqlite.db> --passphrase secret
```

The indexer stores each entry's timestamp, level, message and
correlation identifier in a SQLite database while recording the last
file offset it processed. Subsequent runs resume automatically from the
previous offset, and every insert increments both the
`log_entries_indexed_total` counter and the correlation-tagged
`log_correlation_index_total{correlation_id="…"}` metric for
observability.

Once indexed, the CLI can query for specific correlation IDs, peer
identifiers, transaction hashes or block numbers via:

```
$ contract logs search --db sqlite.db --correlation <id>
$ contract logs search --db sqlite.db --peer peer-42 --since 1700000000 --until 1700003600
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
$ contract logs rotate-key --db sqlite.db --old-passphrase old --new-passphrase new
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