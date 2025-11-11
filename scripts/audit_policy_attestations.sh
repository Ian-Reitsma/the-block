#!/usr/bin/env bash
set -euo pipefail

# Nightly audit: verify signatures on recent policy snapshots
# Emits JSON summary to ${TB_AUDIT_OUT_DIR:-governance/history}/policy_snapshot_attestations_$(date +%F).json

DATA_ROOT="${TB_NODE_DATA_DIR:-node-data}"
POLICY_DIR="${DATA_ROOT}/ad_policy"
OUT_DIR="${TB_AUDIT_OUT_DIR:-governance/history}"
WINDOW="${TB_AUDIT_WINDOW_EPOCHS:-48}"

mkdir -p "${OUT_DIR}"

if [ ! -d "${POLICY_DIR}" ]; then
  echo "policy directory not found: ${POLICY_DIR}" >&2
  exit 0
fi

# Determine latest epoch from *.json stems
latest_epoch() {
  ls -1 "${POLICY_DIR}"/*.json 2>/dev/null | awk -F'/' '{print $NF}' | sed 's/\.json$//' | sort -n | tail -n1
}

M=$(latest_epoch || true)
if [ -z "${M}" ]; then
  echo "no snapshots to verify under ${POLICY_DIR}" >&2
  exit 0
fi

CLI="contract-cli"
if ! command -v "${CLI}" >/dev/null 2>&1; then
  CLI="cargo run -q -p contract-cli --"
fi

timestamp() { date +%s; }

RESULTS=()
failures=0

start=$(( M - WINDOW + 1 ))
if [ "${start}" -lt 0 ]; then start=0; fi

for ((e=M; e>=start; e--)); do
  if [ ! -f "${POLICY_DIR}/${e}.json" ]; then
    # skipped (missing sidecar or payload) -> treat as skipped
    RESULTS+=("{\"epoch\":${e},\"status\":\"skipped\"}")
    continue
  fi
  if [ ! -f "${POLICY_DIR}/${e}.sig" ]; then
    RESULTS+=("{\"epoch\":${e},\"status\":\"skipped\"}")
    continue
  fi
  if ${CLI} ad-market policy verify --data-dir "${DATA_ROOT}" --epoch "${e}" >/dev/null 2>&1; then
    RESULTS+=("{\"epoch\":${e},\"status\":\"ok\"}")
  else
    failures=$(( failures + 1 ))
    # capture brief reason (best-effort)
    reason=$(${CLI} ad-market policy verify --data-dir "${DATA_ROOT}" --epoch "${e}" 2>&1 || true)
    reason=$(echo "$reason" | head -n1 | sed 's/"/\"/g')
    RESULTS+=("{\"epoch\":${e},\"status\":\"failed\",\"reason\":\"${reason}\"}")
  fi
done

results_joined=$(IFS=, ; echo "${RESULTS[*]}")
generated_at=$(timestamp)

echo "{\"generated_at\":${generated_at},\"results\":[${results_joined}]}" > "${OUT_DIR}/policy_snapshot_attestations_$(date +%F).json"

# Optional Prometheus textfile output for each epoch (1 or 0)
if [ -n "${TB_PROM_TEXT_OUT_DIR:-}" ]; then
  mkdir -p "${TB_PROM_TEXT_OUT_DIR}"
  for r in "${RESULTS[@]}"; do
    # parse epoch and status
    ep=$(echo "$r" | sed -n 's/.*"epoch":\([0-9]*\).*/\1/p')
    st=$(echo "$r" | sed -n 's/.*"status":"\([a-z]*\)".*/\1/p')
    val=0
    [ "$st" = "ok" ] && val=1
    echo "ad_policy_attestation_ok_total{epoch=\"${ep}\"} ${val}" > "${TB_PROM_TEXT_OUT_DIR}/ad_policy_attestation_epoch_${ep}.prom"
  done
fi

# Optional webhook on failures
if [ "$failures" -gt 0 ] && [ -n "${GOV_WEBHOOK_URL:-}" ]; then
  summary="${failures} policy snapshot attestation(s) failed on $(date +%F)"
  payload=$(printf '{"text":"%s"}' "${summary}")
  curl -sSf -X POST -H 'Content-Type: application/json' --data "${payload}" "${GOV_WEBHOOK_URL}" || true
fi

exit $([ "$failures" -gt 0 ] && echo 1 || echo 0)

