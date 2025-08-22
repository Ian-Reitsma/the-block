#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MON_DIR="$ROOT_DIR/monitoring"
BIN_DIR="$MON_DIR/bin"
PROM_VERSION="2.51.1"
GRAF_VERSION="11.1.0"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) PROM_ARCH="amd64"; GRAF_ARCH="amd64" ;;
  aarch64|arm64) PROM_ARCH="arm64"; GRAF_ARCH="arm64" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

case "$OS" in
  linux) PROM_OS="linux"; GRAF_OS="linux" ;;
  darwin) PROM_OS="darwin"; GRAF_OS="darwin" ;;
  *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

mkdir -p "$BIN_DIR"

# Download Prometheus
if [[ ! -x "$BIN_DIR/prometheus" ]]; then
  PROM_TAR="prometheus-${PROM_VERSION}.${PROM_OS}-${PROM_ARCH}.tar.gz"
  CHECKS="https://github.com/prometheus/prometheus/releases/download/v${PROM_VERSION}/sha256sums.txt"
  curl -L --fail --show-error "https://github.com/prometheus/prometheus/releases/download/v${PROM_VERSION}/${PROM_TAR}" -o "$BIN_DIR/${PROM_TAR}"
  curl -L --fail --show-error "$CHECKS" -o "$BIN_DIR/sha256sums.txt"
  if command -v sha256sum >/dev/null 2>&1; then
    grep "  ${PROM_TAR}" "$BIN_DIR/sha256sums.txt" | sha256sum -c - || { echo "Prometheus checksum mismatch" >&2; exit 1; }
  else
    grep "  ${PROM_TAR}" "$BIN_DIR/sha256sums.txt" | shasum -a 256 -c - || { echo "Prometheus checksum mismatch" >&2; exit 1; }
  fi
  tar -xzf "$BIN_DIR/${PROM_TAR}" -C "$BIN_DIR"
  mv "$BIN_DIR/prometheus-${PROM_VERSION}.${PROM_OS}-${PROM_ARCH}/prometheus" "$BIN_DIR/prometheus"
  rm -rf "$BIN_DIR/prometheus-${PROM_VERSION}.${PROM_OS}-${PROM_ARCH}" "$BIN_DIR/${PROM_TAR}" "$BIN_DIR/sha256sums.txt"
fi

# Download Grafana
if [[ ! -x "$BIN_DIR/grafana/bin/grafana-server" ]]; then
  GRAF_TAR="grafana-${GRAF_VERSION}.${GRAF_OS}-${GRAF_ARCH}.tar.gz"
  curl -L --fail --show-error "https://dl.grafana.com/oss/release/${GRAF_TAR}" -o "$BIN_DIR/${GRAF_TAR}"
  curl -L --fail --show-error "https://dl.grafana.com/oss/release/${GRAF_TAR}.sha256" -o "$BIN_DIR/${GRAF_TAR}.sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$BIN_DIR" && sha256sum -c "${GRAF_TAR}.sha256") || { echo "Grafana checksum mismatch" >&2; exit 1; }
  else
    (cd "$BIN_DIR" && shasum -a 256 -c "${GRAF_TAR}.sha256") || { echo "Grafana checksum mismatch" >&2; exit 1; }
  fi
  tar -xzf "$BIN_DIR/${GRAF_TAR}" -C "$BIN_DIR"
  mv "$BIN_DIR/grafana-${GRAF_VERSION}" "$BIN_DIR/grafana"
  rm "$BIN_DIR/${GRAF_TAR}" "$BIN_DIR/${GRAF_TAR}.sha256"
fi

GRAF_HOME="$BIN_DIR/grafana"
mkdir -p "$GRAF_HOME/provisioning/dashboards"
cp "$MON_DIR/grafana/dashboard.json" "$GRAF_HOME/provisioning/dashboards/dashboard.json"
cat > "$GRAF_HOME/provisioning/dashboards/dashboard.yml" <<YAML
apiVersion: 1
providers:
  - name: default
    type: file
    options:
      path: $GRAF_HOME/provisioning/dashboards
YAML

"$BIN_DIR/prometheus" --config.file="$MON_DIR/prometheus.yml" &
PROM_PID=$!
"$GRAF_HOME/bin/grafana-server" --homepath "$GRAF_HOME" >/dev/null 2>&1 &
GRAF_PID=$!

echo "Prometheus running on http://localhost:9090 (PID $PROM_PID)"
echo "Grafana running on http://localhost:3000 (PID $GRAF_PID)"

disable() {
  kill "$PROM_PID" "$GRAF_PID" 2>/dev/null || true
}
trap disable EXIT
wait
