#!/usr/bin/env bash
set -euo pipefail
VERSION=${FSTAR_VERSION:-v2025.08.07}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/formal/.fstar"
TARBALL="fstar-${VERSION}-Linux-x86_64.tar.gz"
HOME_DIR="$DIR/fstar"
BIN="$HOME_DIR/bin/fstar.exe"
if [ ! -x "$BIN" ]; then
  mkdir -p "$DIR"
  echo "Downloading F* $VERSION" >&2
  curl -L "https://github.com/FStarLang/FStar/releases/download/${VERSION}/${TARBALL}" -o "$DIR/${TARBALL}"
  tar -xf "$DIR/${TARBALL}" -C "$DIR"
  rm "$DIR/${TARBALL}"
fi
echo "$HOME_DIR"
