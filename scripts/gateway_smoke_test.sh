#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: gateway_smoke_test.sh [GATEWAY_URL] [BLOCK_DOMAIN] [DOH_HOST]

  GATEWAY_URL  Base URL for the gateway (default: http://127.0.0.1:9000)
  BLOCK_DOMAIN Domain whose content and resolver should be exercised (default: example.block)
  DOH_HOST     Optional host header for /dns/resolve (default: extracted from GATEWAY_URL)

The script hits /, /dns/resolve, and then checks the current read-ack file under
TB_GATEWAY_ACK_DIR (default gateway_acks) to ensure read acknowledgements are
persisted for the requested domain.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    usage
    exit 0
fi

gateway_url="${1:-http://127.0.0.1:9000}"
domain="${2:-example.block}"
gateway_host="${3:-}"

extract_host() {
    local url="${1#*://}"
    printf '%s' "${url%%/*}"
}

resolver_host="${gateway_host:-$(extract_host "$gateway_url")}"
ack_dir="${TB_GATEWAY_ACK_DIR:-gateway_acks}"
epoch=$(( $(date -u +%s) / 3600 ))
ack_file="${ack_dir}/${epoch}.jsonl"

printf 'Gateway smoke test → gateway=%s domain=%s resolver_host=%s ack_dir=%s\n' \
    "$gateway_url" "$domain" "$resolver_host" "$ack_dir"

printf '  · fetching content for host=%s\n' "$domain"
curl --fail --silent --show-error -H "Host: ${domain}" "${gateway_url}/" >/dev/null

printf '  · querying resolver for %s\n' "$domain"
curl --fail --silent --show-error -H "Host: ${resolver_host}" \
    "${gateway_url}/dns/resolve?name=${domain}&type=A" | grep -q '"Status":[[:space:]]*0'

sleep 1

if [[ ! -f "$ack_file" ]]; then
    echo "Read-ack file not found: $ack_file" >&2
    exit 1
fi

tail -n1 "$ack_file" | grep -q "\"domain\":\"${domain}\"" || {
    echo "Read-ack file $ack_file does not contain domain ${domain}" >&2
    exit 1
}

echo "Read ack recorded in $ack_file"
