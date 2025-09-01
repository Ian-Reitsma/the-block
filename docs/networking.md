# Networking Recovery Guide

This guide describes how to restore the distributed hash table (DHT) state when the peer database becomes corrupt or unreachable.

## Resetting the Peer Database
1. **Stop the node** to avoid concurrent writes.
2. Remove the persisted peer list:
   ```bash
   rm ~/.the_block/peers.txt
   ```
   Alternatively point the node at a fresh location by exporting `TB_PEER_DB_PATH`.
3. Optionally pin bootstrap order for tests with:
   ```bash
   export TB_PEER_SEED=1
   ```

## Bootstrapping
1. Start the node and supply at least one known-good peer address:
   ```bash
   cargo run -p the_block --bin node -- run --rpc_addr 127.0.0.1:3030 \
       --data-dir node-data
   ```
   Then edit `~/.the_block/peers.txt` and add `ip:port` entries for trusted peers.  The node will randomize the list on startup.
2. Verify connectivity using the ban utility:
   ```bash
   cargo run -p the_block --bin ban -- --list
   ```
   This prints the current peer set and allows manual removal with `--remove <ip:port>`.
3. Check for handshake failures and DHT convergence via metrics:
   ```bash
   curl -s localhost:9100/metrics | rg '^dht_peers_total'
   ```
   A steadily increasing peer count after bootstrap indicates healthy gossip.

## Recovery After Corruption
If the peer file was truncated or contained invalid IDs, the discovery layer may misbehave.  After deleting the file and supplying fresh peers as above, restart the node.  The DHT will rebuild automatically and persist the updated peer list on clean shutdown.

These steps can be repeated on any node to recover from corrupted peer databases or during network bootstrapping.

## RPC Client Timeouts

RPC clients stagger their request retries to avoid thundering herds.  The
following environment variables control timeout behaviour:

- `TB_RPC_TIMEOUT_MS` – base timeout per request (default 5000ms)
- `TB_RPC_TIMEOUT_JITTER_MS` – additional random jitter added to each timeout
  (default 1000ms)
- `TB_RPC_MAX_RETRIES` – number of retry attempts on timeout (default 3)

Set these variables to tune client behaviour in constrained or high latency
networks.

## Fuzzing Peer Identifiers

Malformed peer identifiers should never crash or mis-route. Run the fuzz harness
under `net/fuzz/` to stress the parser:

```bash
RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="net/fuzz/peer_id-%p.profraw" \
  cargo +nightly fuzz run peer_id --fuzz-dir net/fuzz -- -runs=100
scripts/fuzz_coverage.sh /tmp/net_cov
```

The coverage script installs missing LLVM tools automatically and merges any
generated `.profraw` files into an HTML report.
