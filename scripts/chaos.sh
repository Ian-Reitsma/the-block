#!/usr/bin/env bash
set -euo pipefail
N=5
DUR=120
PART=""
KILL=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --nodes) N="$2"; shift 2 ;;
    --duration) DUR="$2"; shift 2 ;;
    --partition) PART="$2"; shift 2 ;;
    --kill) KILL="$2"; shift 2 ;;
    --quic-loss) export TB_QUIC_PACKET_LOSS="$2"; shift 2 ;;
    --quic-dup) export TB_QUIC_PACKET_DUP="$2"; shift 2 ;;
    *) echo "usage: $0 [--nodes N] [--duration SECS] [--partition IDX@START-END] [--kill IDX@TIME] [--quic-loss P] [--quic-dup P]" >&2; exit 1 ;;
  esac
done
schedule=$(mktemp)
echo "nodes=$N" > "$schedule"
echo "duration=$DUR" >> "$schedule"
[[ -n "$PART" ]] && echo "partition=$PART" >> "$schedule"
[[ -n "$KILL" ]] && echo "kill=$KILL" >> "$schedule"
trap 'rm -f "$schedule"' EXIT
scripts/swarm.sh up
start=$(date +%s)
if [[ -n "$KILL" ]]; then
  idx=${KILL%@*}
  t=${KILL#*@}
  (
    sleep "$t"
    if [[ -f swarm/pids/node$idx.pid ]]; then
      kill -9 "$(cat swarm/pids/node$idx.pid)" 2>/dev/null || true
    fi
  ) &
fi
if [[ -n "$PART" ]]; then
  idx=${PART%@*}
  rng=${PART#*@}
  start_t=${rng%-*}
  end_t=${rng#*-}
  port=$((35000 + 100 + idx))
  (
    sleep "$start_t"
    iptables -I INPUT -p tcp --sport "$port" -j DROP 2>/dev/null || true
    iptables -I OUTPUT -p tcp --dport "$port" -j DROP 2>/dev/null || true
    iptables -I INPUT -p udp --sport "$port" -j DROP 2>/dev/null || true
    iptables -I OUTPUT -p udp --dport "$port" -j DROP 2>/dev/null || true
    sleep $((end_t - start_t))
    iptables -D INPUT -p tcp --sport "$port" -j DROP 2>/dev/null || true
    iptables -D OUTPUT -p tcp --dport "$port" -j DROP 2>/dev/null || true
    iptables -D INPUT -p udp --sport "$port" -j DROP 2>/dev/null || true
    iptables -D OUTPUT -p udp --dport "$port" -j DROP 2>/dev/null || true
  ) &
fi
sleep "$DUR"
scripts/swarm.sh down
tar -czf chaos_artifacts.tar.gz swarm/logs >/dev/null 2>&1
