#!/bin/bash
cd /Users/ianreitsma/projects/the-block
grep -r "partition_heals_to_majority" node/tests/ 2>/dev/null || echo "Not found in node/tests"
grep -r "partition_heals_to_majority" tests/ 2>/dev/null || echo "Not found in tests/"
grep -r "kill_node_recovers" node/tests/ 2>/dev/null || echo "Not found in node/tests"
grep -r "kill_node_recovers" tests/ 2>/dev/null || echo "Not found in tests/"
