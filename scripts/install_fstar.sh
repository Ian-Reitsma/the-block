#!/usr/bin/env bash
set -euo pipefail
VERSION=${FSTAR_VERSION:-v2025.08.07}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/formal/.fstar/$VERSION"
BIN="$DIR/bin/fstar.exe"
if [ ! -x "$BIN" ]; then
  OS_RAW=$(uname -s)
  ARCH_RAW=$(uname -m)
  case "$OS_RAW" in
    Linux|Darwin) ;; 
    *) echo "Unsupported OS: $OS_RAW" >&2; exit 1 ;;
  esac
  if [ "$OS_RAW" = "Linux" ]; then
    case "$ARCH_RAW" in
      x86_64|amd64) ARCH="x86_64" ;;
      aarch64|arm64) ARCH="aarch64" ;;
      *) echo "Unsupported arch: $ARCH_RAW" >&2; exit 1 ;;
    esac
  else # Darwin
    case "$ARCH_RAW" in
      x86_64|amd64) ARCH="x86_64" ;;
      arm64|aarch64) ARCH="arm64" ;;
      *) echo "Unsupported arch: $ARCH_RAW" >&2; exit 1 ;;
    esac
  fi
  mkdir -p "$DIR"
  TARBALL="fstar-${VERSION}-${OS_RAW}-${ARCH}.tar.gz"
  URL="https://github.com/FStarLang/FStar/releases/download/${VERSION}/${TARBALL}"
  if ! curl --head --fail --silent "$URL" >/dev/null; then
    echo "F★ release $VERSION not found" >&2
    exit 1
  fi
  if ! curl -L --fail --show-error "$URL" -o "$DIR/$TARBALL"; then
    echo "Failed to download $URL" >&2
    exit 1
  fi
  if ! curl -L --fail --show-error "$URL.sha256" -o "$DIR/$TARBALL.sha256"; then
    echo "Failed to download checksum" >&2
    exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$DIR" && echo "$(cat $TARBALL.sha256)  $TARBALL" | sha256sum -c -)
  else
    (cd "$DIR" && echo "$(cat $TARBALL.sha256)  $TARBALL" | shasum -a 256 -c -)
  fi
  tar -xf "$DIR/$TARBALL" -C "$DIR"
  rm "$DIR/$TARBALL" "$DIR/$TARBALL.sha256"
fi
echo "$DIR"
