#!/bin/bash
cd /Users/ianreitsma/projects/the-block

echo "=== Searching for Node peer management methods ==="
grep -n "fn add_peer\|fn remove_peer\|fn broadcast_chain\|fn discover_peers\|fn clear_peers" node/src/net/mod.rs

echo "
=== Searching for blockchain lock usage ==="
grep -n "blockchain()" node/tests/chaos.rs | head -20

echo "
=== Finding mutex/rwlock usage in peer management ==="
grep -n "lock_mutex\|read_lock\|write_lock" node/src/net/mod.rs | head -30
