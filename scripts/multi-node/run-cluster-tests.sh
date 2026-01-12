#!/usr/bin/env bash
set -euo pipefail

# Run the multi-node RPC smoke test against a live 3-node cluster.
# Expects TB_MULTI_NODE_RPC to list the RPC endpoints, e.g.:
#   TB_MULTI_NODE_RPC=192.168.1.10:3030,192.168.1.11:4030,192.168.1.12:5030

if [[ -z "${TB_MULTI_NODE_RPC:-}" ]]; then
  echo "TB_MULTI_NODE_RPC not set (comma-separated rpc hosts:ports)" >&2
  exit 1
fi

echo "Running multi-node RPC smoke test against ${TB_MULTI_NODE_RPC}"
cargo test -p the_block --all-features --test multi_node_rpc -- --nocapture
