#!/usr/bin/env bash
set -euo pipefail
CMD=${1:-}
SWARM_DIR=${SWARM_DIR:-swarm}
N=${SWARM_NODES:-5}
BASE=${SWARM_BASE:-35000}
PROM_PORT=${PROM_PORT:-$((BASE+300))}
GRAF_PORT=${GRAF_PORT:-$((BASE+400))}

case "$CMD" in
  up)
    mkdir -p "$SWARM_DIR/logs" "$SWARM_DIR/pids"
    for i in $(seq 1 $N); do
      dir="$SWARM_DIR/node$i"
      mkdir -p "$dir"
      rpc=$((BASE + i))
      p2p=$((BASE + 100 + i))
      prom=$((BASE + 200 + i))
      fb=$((1 << ((i-1) % 3)))
      sed -e "s/__RPC_PORT__/$rpc/" \
          -e "s/__P2P_PORT__/$p2p/" \
          -e "s/__PROM_PORT__/$prom/" \
          -e "s/__NODE_NAME__/node$i/" \
          -e "s/__FEATURE_BITS__/0x$(printf '%x' $fb)/" \
          scripts/templates/config.toml.tpl > "$dir/config.toml"
      ./target/release/node --config "$dir/config.toml" >"$SWARM_DIR/logs/node$i.log" 2>&1 &
      echo $! > "$SWARM_DIR/pids/node$i.pid"
    done
    prometheus --config.file=monitoring/prometheus.yml --web.listen-address=":$PROM_PORT" >"$SWARM_DIR/logs/prometheus.log" 2>&1 &
    echo $! > "$SWARM_DIR/pids/prometheus.pid"
    if command -v grafana-server >/dev/null 2>&1; then
      grafana-server --homepath monitoring/grafana --config monitoring/grafana/grafana.ini --port "$GRAF_PORT" >"$SWARM_DIR/logs/grafana.log" 2>&1 &
      echo $! > "$SWARM_DIR/pids/grafana.pid"
    fi
    echo "Prometheus: http://localhost:$PROM_PORT"
    echo "Grafana: http://localhost:$GRAF_PORT"
    ;;
  down)
    if [ -d "$SWARM_DIR/pids" ]; then
      for pid in "$SWARM_DIR"/pids/*.pid; do
        [ -e "$pid" ] || continue
        kill "$(cat "$pid")" 2>/dev/null || true
        rm "$pid"
      done
    fi
    if [ -d "$SWARM_DIR/logs" ]; then
      ts=$(date +%Y%m%d-%H%M)
      mkdir -p "$SWARM_DIR/artifacts"
      tar -czf "$SWARM_DIR/artifacts/$ts.tar.gz" -C "$SWARM_DIR" logs
    fi
    ;;
  logs)
    tail -F "$SWARM_DIR"/logs/*.log
    ;;
  chaos)
    while true; do
      sleep 30
      idx=$(( (RANDOM % N) + 1 ))
      pidfile="$SWARM_DIR/pids/node$idx.pid"
      if [ -f "$pidfile" ]; then
        kill "$(cat "$pidfile")" 2>/dev/null || true
        rpc=$((BASE + idx))
        p2p=$((BASE + 100 + idx))
        prom=$((BASE + 200 + idx))
        dir="$SWARM_DIR/node$idx"
        ./target/release/node --config "$dir/config.toml" >"$SWARM_DIR/logs/node$idx.log" 2>&1 &
        echo $! > "$pidfile"
      fi
    done
    ;;
  *)
    echo "usage: $0 {up|down|logs|chaos}" >&2
    exit 1
    ;;
esac
