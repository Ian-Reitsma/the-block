#!/usr/bin/env bash
set -euo pipefail
VERSION=${FSTAR_VERSION:-v2025.08.07}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Allow callers to reuse an existing installation by setting FSTAR_HOME.
if [ -n "${FSTAR_HOME:-}" ] && [ -x "$FSTAR_HOME/bin/fstar.exe" ]; then
  export FSTAR_HOME
  echo "$FSTAR_HOME"
  exit 0
fi

FSTAR_HOME="$ROOT/formal/.fstar/$VERSION"
BIN="$FSTAR_HOME/bin/fstar.exe"
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
  mkdir -p "$FSTAR_HOME"
    TARBALL="fstar-${VERSION}-${OS_RAW}-${ARCH}.tar.gz"
    URL="https://github.com/FStarLang/FStar/releases/download/${VERSION}/${TARBALL}"
    if ! curl --head --fail --silent "$URL" >/dev/null; then
      echo "F★ release $VERSION not found" >&2
      exit 1
    fi
    if ! curl -L --fail --show-error "$URL" -o "$FSTAR_HOME/$TARBALL"; then
      echo "Failed to download $URL" >&2
      exit 1
    fi
    DIGEST=$(curl -fsSL "https://api.github.com/repos/FStarLang/FStar/releases/tags/${VERSION}" | jq -r ".assets[] | select(.name==\"$TARBALL\") | .digest" | cut -d: -f2)
    if [ -n "$DIGEST" ]; then
      if command -v sha256sum >/dev/null 2>&1; then
        (cd "$FSTAR_HOME" && echo "$DIGEST  $TARBALL" | sha256sum -c -)
      else
        (cd "$FSTAR_HOME" && echo "$DIGEST  $TARBALL" | shasum -a 256 -c -)
      fi
    else
      echo "warning: no checksum available for $TARBALL; skipping verification" >&2
    fi
    tar -xf "$FSTAR_HOME/$TARBALL" -C "$FSTAR_HOME"
    rm "$FSTAR_HOME/$TARBALL"
    if [ -d "$FSTAR_HOME/fstar" ]; then
      mv "$FSTAR_HOME/fstar"/* "$FSTAR_HOME"/
      rmdir "$FSTAR_HOME/fstar"
    fi
fi
export FSTAR_HOME
echo "$FSTAR_HOME"
