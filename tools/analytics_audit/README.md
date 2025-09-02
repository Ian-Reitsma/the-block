# analytics_audit

Utility to verify `ReadAck` batches against on-chain totals.

```bash
cargo run -p analytics_audit -- path/to/batch.cbor
```

The tool recomputes the Merkle root and total bytes of a CBOR batch file
and prints them for comparison with the `read_root` and `read_sub_ct`
fields recorded in the corresponding block.
