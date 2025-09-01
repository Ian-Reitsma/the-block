# Network Bootstrapping and Recovery

Peers discovered by the gossip layer are persisted to `~/.the_block/peers.txt`.
On startup the node merges these records with any statically configured
addresses and shuffles the list before attempting handshakes.  The shuffle seed
can be fixed by setting `TB_PEER_SEED` for reproducible bootstrap tests.

If a node fails to connect due to stale or corrupt entries, remove the file or
point `TB_PEER_DB_PATH` to a fresh location and restart.  Handshake rejections
are exported via the `peer_handshake_failure_total{reason="*"}` metric to aid
monitoring and manual recovery.
