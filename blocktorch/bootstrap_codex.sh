#!/usr/bin/env bash
set -euo pipefail

APP_NAME="metal-orchard"
REPO_OWNER="Ian-Reitsma"
REPO_NAME="metal-orchard"

AGENT_SECRET_VAR="AGENT_METAL_Codex_Aug25"  # GitHub PAT (Codex Secret)

TARGET_BRANCH="${TARGET_BRANCH:-agent/codex}"
RUN_LOCAL_BUILD="${RUN_LOCAL_BUILD:-0}"
REMOTE_MAC_SSH="${REMOTE_MAC_SSH:-agent_codex_the-block}"
REMOTE_MAC_PATH="${REMOTE_MAC_PATH:-/Users/ianreitsma/projects/metal-orchard}"
REMOTE_RUN_BUILD="${REMOTE_RUN_BUILD:-0}"

say(){ printf '%s\n' "$*"; }
die(){ printf 'ERROR: %s\n' "$*" >&2; exit 1; }
need(){ command -v "$1" >/dev/null 2>&1 || die "missing: $1"; }

trap 'echo "[FATAL] ${APP_NAME} failed at line $LINENO" >&2' ERR
cd /workspace/metal-orchard

[[ -n "${!AGENT_SECRET_VAR:-}" ]] || die "$AGENT_SECRET_VAR not set"
need git
[[ "$RUN_LOCAL_BUILD" != "1" ]] || { need cmake; need ninja; }
need ssh; need rsync

MAIN_REMOTE_URL="https://${!AGENT_SECRET_VAR}@github.com/${REPO_OWNER}/${REPO_NAME}.git"
if ! git remote | grep -qx origin; then git remote add origin "$MAIN_REMOTE_URL"; else git remote set-url origin "$MAIN_REMOTE_URL"; fi

say "[git] fetch $TARGET_BRANCH"
git fetch --no-tags origin "$TARGET_BRANCH" || die "branch $TARGET_BRANCH not found"
git checkout -q "$TARGET_BRANCH"
git pull --ff-only origin "$TARGET_BRANCH"
BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo DETACHED)"
[[ "$BRANCH" == "$TARGET_BRANCH" ]] || die "checked out $BRANCH, expected $TARGET_BRANCH"

if [[ "$RUN_LOCAL_BUILD" == "1" ]]; then
  say "[local] cmake + tests"
  cmake -S . -B build -G Ninja -DFETCHCONTENT_FULLY_DISCONNECTED=ON
  cmake --build build --target check
fi

say "[remote] ensure ${REMOTE_MAC_SSH}:${REMOTE_MAC_PATH}"
ssh -o BatchMode=yes "$REMOTE_MAC_SSH" bash -lc "mkdir -p '${REMOTE_MAC_PATH}'"
say "[remote] rsync → ${REMOTE_MAC_SSH}:${REMOTE_MAC_PATH}"
rsync -az --delete --exclude '.git' --exclude '.github' --exclude 'build' ./ "$REMOTE_MAC_SSH:${REMOTE_MAC_PATH}/"

if [[ "$REMOTE_RUN_BUILD" == "1" ]]; then
  say "[remote] cmake + tests"
  ssh -o BatchMode=yes "$REMOTE_MAC_SSH" bash -lc "
    set -euo pipefail
    cd '${REMOTE_MAC_PATH}'
    cmake -S . -B build -G Ninja -DFETCHCONTENT_FULLY_DISCONNECTED=ON
    cmake --build build --target check
  "
  say "[remote] build/tests complete"
else
  say "[remote] sync complete (build skipped)"
fi

say "=== [$APP_NAME] BOOTSTRAP DONE ==="
