#!/usr/bin/env bash
# Notify operators of policy updates.
set -euo pipefail
POLICY_URL=${1:-}
if [[ -z "$POLICY_URL" ]]; then
  echo "usage: $0 <policy-feed-url>" >&2
  exit 1
fi
curl -fsSL "$POLICY_URL" | jq '.region' && echo "policy update fetched" || echo "failed"
