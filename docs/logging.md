# Log Correlation and Search

Structured logs include a per-request `correlation_id` field that links
individual log entries with telemetry metrics and external tooling.

Logs emitted as JSON can be indexed with the `log_indexer` utility:

```
$ cargo run --bin log_indexer -- index <logfile> <sqlite.db>
$ cargo run --bin log_indexer -- index <logfile> <sqlite.db> --passphrase secret
```

The indexer stores each entry's timestamp, level, message and
correlation identifier in a SQLite database.  It also increments the
`log_entries_indexed_total` metric for observability.

Once indexed, the CLI can query for specific correlation IDs, peer
identifiers, transaction hashes or block numbers via:

```
$ contract logs search --db sqlite.db --correlation <id>
$ contract logs search --db sqlite.db --peer peer-42 --passphrase secret
```

Encrypted messages are decrypted on the fly when the same passphrase is
supplied. Messages without a passphrase are displayed as `<encrypted>`
so operators can still correlate telemetry without exposing payloads.
