#!/bin/bash
cd ~/projects/the-block
echo "=== Testing Receipt Integration ==="
cargo test --test receipt_integration 2>&1