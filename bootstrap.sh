#!/usr/bin/env bash
# === ultimate bootstrap (never abort, always report, always suggest fix) ===

if [[ -z "$BASH_VERSION" ]]; then
  echo "This script must be run in bash. Run as: bash $0"
  exit 1
fi

cecho() {
  local color="$1"; shift
  if [[ -t 1 ]]; then
    case "$color" in
      red) tput setaf 1;;
      green) tput setaf 2;;
      yellow) tput setaf 3;;
      blue) tput setaf 4;;
      cyan) tput setaf 6;;
    esac
    echo -e "$*"
    tput sgr0
  else
    echo "$*"
  fi
}

set -Euo pipefail
IFS=$'\n\t'

APP_NAME="the-block"
REQUIRED_PYTHON="3.12.3"
PARTIAL_RUN_FLAG=".bootstrap_partial"
touch "$PARTIAL_RUN_FLAG"

NATIVE_MONITOR=0
for arg in "$@"; do
  [[ "$arg" == "--native-monitor" ]] && NATIVE_MONITOR=1
done

FAILED_STEPS=()
SKIPPED_STEPS=()
FIX_COMMANDS=()
BROKEN_PYTHON=0
PY_SHIM=0

trap 'cleanup; exit 1' SIGINT SIGTERM

cleanup() {
  if [[ -f "$PARTIAL_RUN_FLAG" ]]; then
    cecho red "[ABORTED] Cleaning up partial bootstrap/build artifacts..."
    for dir in build dist __pycache__ .pytest_cache .mypy_cache .tox CMakeFiles pip-wheel-metadata; do
      [[ -d $dir ]] && rm -rf "$dir" && cecho yellow "  → Removed $dir"
    done
    for file in CMakeCache.txt poetry.lock package-lock.json pnpm-lock.yaml yarn.lock bootstrap.log; do
      [[ -f $file && ! -f package.json ]] && rm -f "$file" && cecho yellow "  → Removed orphaned $file"
    done
    cecho red "[CLEANUP DONE] Exiting due to error/interruption."
  fi
}

run_step() {
  local desc="$1"; shift
  cecho cyan "→ $desc"
  set +e
  "$@"
  local status=$?
  set -e
  if [[ $status -ne 0 ]]; then
    cecho red "   ✗ $desc failed (exit $status)"
    FAILED_STEPS+=("$desc | $*")
    return $status
  else
    cecho green "   ✓ ok"
  fi
}

skip_step() {
  local desc="$1"
  cecho yellow "   → $desc skipped"
  SKIPPED_STEPS+=("$desc")
}

# OS detection and guard
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

if [[ "$OS" =~ (mingw|msys|cygwin) ]]; then
  cecho yellow "🪟  Native Windows shell detected."
  cecho yellow "   ➤  This script is for Linux/macOS/WSL2. To bootstrap on native Windows:"
  cecho yellow "      - Use Windows Subsystem for Linux 2 (WSL2) and run this script inside Ubuntu/Fedora/etc."
  cecho yellow "      - Or use Git-Bash (with caveats: venv/rust/python interop is not fully supported)."
  cecho yellow "      - For true native setup, run or adapt 'bootstrap.ps1' (PowerShell version, coming soon)."
  cecho yellow "      - Install Python, Node, Rust, Maturin, and dependencies via Chocolatey or Scoop."
  skip_step "Windows shell (not supported natively)"
  rm -f "$PARTIAL_RUN_FLAG"
  exit 0
fi

# Optionally, warn if user is on WSL1, which is not recommended:
if grep -q "Microsoft" /proc/version 2>/dev/null && ! grep -q "WSL2" /proc/version 2>/dev/null; then
  cecho yellow "⚠  Detected WSL1 (not WSL2). Upgrade to WSL2 for full compatibility and performance!"
fi

# .env sync (idempotent)
if [[ -f .env.example ]]; then
  [[ -f .env ]] || cp .env.example .env
  missing=$(comm -23 <(grep -v '^#' .env.example | cut -d= -f1 | sort) <(grep -v '^#' .env | cut -d= -f1 | sort)) || true
  for k in ${missing:-}; do grep "^$k=" .env.example >> .env; cecho green "   + added env key $k"; done
else
  cecho yellow "   → No .env.example found, skipping env sync."
fi

# requirements.txt/package.json sanity
if [[ -f requirements.txt ]]; then
  if [[ ! -s requirements.txt ]] || ! grep -q '[^[:space:]]' requirements.txt; then
    cecho yellow "   → requirements.txt present but empty; skipping pip install."
    SKIPPED_STEPS+=("requirements.txt present but empty, pip install skipped")
    rm -f requirements.txt
  fi
fi

if [[ -f package.json ]]; then
  if [[ ! -s package.json ]] || ! grep -q '{' package.json; then
    cecho yellow "   → package.json present but empty/invalid; rewriting as '{}'."
    echo '{}' > package.json
    SKIPPED_STEPS+=("package.json fixed to valid '{}'")
  elif ! node -e 'require("./package.json")' 2>/dev/null; then
    cecho red "   → package.json is invalid JSON. Please fix it."
    FAILED_STEPS+=("package.json invalid, npm will fail until corrected")
    FIX_COMMANDS+=("echo '{}' > package.json  # fix package.json")
  fi
fi

# system dependencies
install_deps_apt() {
  run_step "apt-get update" timeout 60s sudo apt-get update
  run_step "apt-get install build deps" sudo apt-get install -y build-essential zlib1g-dev libffi-dev libssl-dev \
      libbz2-dev libreadline-dev libsqlite3-dev curl git jq lsof pkg-config \
      python3 python3-venv python3-pip cmake make unzip
}
install_deps_dnf() {
  run_step "dnf install nodejs/npm" sudo dnf install -y nodejs npm
  run_step "dnf install build deps" sudo dnf install -y gcc gcc-c++ make openssl-devel zlib-devel readline-devel \
      curl git jq lsof pkg-config patch cmake sqlite-devel bzip2-devel xz-devel libffi-devel tk-devel unzip
  run_step "dnf install python3" sudo dnf install -y python3 python3-pip python3-virtualenv
  if ! python3 --version 2>/dev/null | grep -q '3\.12'; then
    cecho yellow "⚠  Fedora does not ship python3.12 as a separate package. You have: $(python3 --version)"
    SKIPPED_STEPS+=("python3.12: Fedora ships only python3.x, not python3.12.x")
  fi
}
install_deps_brew() {
  if ! command -v brew &>/dev/null; then
    run_step "install Homebrew" /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    eval "$(/opt/homebrew/bin/brew shellenv)"
  fi
  run_step "brew update" brew update
  run_step "brew install build deps" brew install git jq lsof pkg-config openssl@3 python@3.12 cmake make unzip
}

case "$OS" in
  linux)  command -v apt-get &>/dev/null && install_deps_apt
          command -v dnf     &>/dev/null && install_deps_dnf ;;
  darwin) install_deps_brew ;;
esac

if ! command -v python &>/dev/null; then
  PY_SHIM=1
fi

# python & venv, pyenv fallback
PY_BIN=""
if command -v python3.12 &>/dev/null; then
  PY_BIN="$(command -v python3.12)"
elif python3 --version 2>/dev/null | grep -q '3\.12'; then
  PY_BIN="$(command -v python3)"
elif python3 --version 2>/dev/null | awk '{print $2}' | grep -qE '^3\.(1[2-9]|[2-9][0-9])'; then
  PY_BIN="$(command -v python3)"
elif command -v conda &>/dev/null && conda info --envs | grep -q '3\.12'; then
  cecho cyan "   → Using Python 3.12 from Conda environment."
  PY_BIN="$(conda run which python)"
else
  if ! command -v pyenv &>/dev/null; then
    run_step "install pyenv" curl https://pyenv.run | bash
  fi
  pyenv_init() {
    export PATH="$HOME/.pyenv/bin:$PATH"
    eval "$(pyenv init -)"
    eval "$(pyenv virtualenv-init -)"
  }
  pyenv_init
  run_step "pyenv install $REQUIRED_PYTHON" pyenv install -s "$REQUIRED_PYTHON"
  pyenv_init
  run_step "pyenv local $REQUIRED_PYTHON" pyenv local "$REQUIRED_PYTHON"
  pyenv_init
  PY_BIN="$(pyenv which python)"
fi

[[ -d .venv ]] || run_step "python -m venv" "$PY_BIN" -m venv .venv
export PATH="$PWD/.venv/bin:$PATH"
if (( PY_SHIM == 1 )); then
  mkdir -p bin
  ln -sf "$(pwd)/.venv/bin/python" bin/python
  export PATH="$PWD/bin:$PATH"
fi
source .venv/bin/activate
export PYO3_PYTHON="$(pwd)/.venv/bin/python"
hash -r
cecho cyan "   project python on PATH; python demo.py works without activation"
if [[ -z "${VIRTUAL_ENV:-}" || "$(command -v python)" != "$(pwd)/.venv/bin/python" ]]; then
  cecho red "Python interpreter mismatch. Activate the project's venv first."
  exit 1
fi

run_step "pip upgrade" python -m pip install --upgrade pip setuptools wheel

# Python _sqlite3 build check
if ! python -c 'import sqlite3' 2>/dev/null; then
  cecho red "   → Python is missing sqlite3/_sqlite3 support. This WILL break pre-commit and pip tools."
  BROKEN_PYTHON=1
  if [[ "$OS" == "linux" && $(command -v dnf) ]]; then
    cecho yellow "      [Fedora/RHEL]: Run: sudo dnf install sqlite-devel bzip2-devel xz-devel libffi-devel tk-devel && pyenv uninstall $REQUIRED_PYTHON && pyenv install $REQUIRED_PYTHON"
    FIX_COMMANDS+=("sudo dnf install sqlite-devel bzip2-devel xz-devel libffi-devel tk-devel")
    FIX_COMMANDS+=("export PATH=\"\$HOME/.pyenv/bin:\$PATH\"; eval \"\$(pyenv init -)\"; eval \"\$(pyenv virtualenv-init -)\"; pyenv uninstall $REQUIRED_PYTHON && PYTHON_CONFIGURE_OPTS='--without-tk' pyenv install $REQUIRED_PYTHON")
  elif [[ "$OS" == "linux" && $(command -v apt-get) ]]; then
    cecho yellow "      [Ubuntu/Debian]: Run: sudo apt-get install libsqlite3-dev && pyenv uninstall $REQUIRED_PYTHON && pyenv install $REQUIRED_PYTHON"
    FIX_COMMANDS+=("sudo apt-get install libsqlite3-dev")
    FIX_COMMANDS+=("export PATH=\"\$HOME/.pyenv/bin:\$PATH\"; eval \"\$(pyenv init -)\"; eval \"\$(pyenv virtualenv-init -)\"; pyenv uninstall $REQUIRED_PYTHON && pyenv install $REQUIRED_PYTHON")
  fi
  SKIPPED_STEPS+=("Python missing sqlite3; re-install with headers to fix")
fi

# Rust toolchain, just, cargo-make, maturin
if ! command -v cargo &>/dev/null; then
  run_step "install Rust" curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi
command -v just >/dev/null 2>&1 || run_step "cargo install just" cargo install just

CARGO_MAKE_VERSION=0.37.24
if cargo make --version 2>/dev/null | grep -q "$CARGO_MAKE_VERSION"; then
  cecho green "   ✓ cargo-make already installed"
else
  arch=$(uname -m)
  os=$(uname -s)
  case "${arch}-${os}" in
    x86_64-Linux)
      pkg="cargo-make-v${CARGO_MAKE_VERSION}-x86_64-unknown-linux-gnu.zip"
      ;;
    aarch64-Linux)
      pkg="cargo-make-v${CARGO_MAKE_VERSION}-aarch64-unknown-linux-gnu.zip"
      ;;
    *)
      cecho red "unsupported architecture: ${arch}-${os}"
      exit 1
      ;;
  esac
  url="https://github.com/sagiegurari/cargo-make/releases/download/${CARGO_MAKE_VERSION}/${pkg}"
  tmpdir=$(mktemp -d)
  run_step "download cargo-make" curl -Ls "$url" -o "$tmpdir/$pkg"
  bindir="${CARGO_HOME:-$HOME/.cargo}/bin"
  mkdir -p "$bindir"
  run_step "install cargo-make" unzip -j "$tmpdir/$pkg" -d "$bindir"
  chmod +x "$bindir/cargo-make"
fi

NEXTEST_VERSION=0.9.102
if cargo nextest --version 2>/dev/null | grep -q "$NEXTEST_VERSION"; then
  cecho green "   ✓ cargo-nextest already installed"
else
  arch=$(uname -m)
  os=$(uname -s)
  case "${arch}-${os}" in
    x86_64-Linux)
      pkg="cargo-nextest-${NEXTEST_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
      ;;
    aarch64-Linux)
      pkg="cargo-nextest-${NEXTEST_VERSION}-aarch64-unknown-linux-gnu.tar.gz"
      ;;
    *)
      cecho red "unsupported architecture: ${arch}-${os}"
      exit 1
      ;;
  esac
  url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/${pkg}"
  tmpdir=$(mktemp -d)
  run_step "download cargo-nextest" bash -c "curl -Ls '$url' | tar -xz -C '$tmpdir'"
  bindir="${CARGO_HOME:-$HOME/.cargo}/bin"
  mkdir -p "$bindir"
  run_step "install cargo-nextest" mv "$tmpdir/cargo-nextest" "$bindir/"
  chmod +x "$bindir/cargo-nextest"
fi

# Only install maturin/pip if Python build is not broken
if (( BROKEN_PYTHON == 0 )); then
  run_step "pip install maturin" pip install --upgrade maturin
else
  skip_step "maturin/pip install (Python broken: sqlite3 missing)"
fi

# Run database migrations and compaction now that Rust and Python are ready
if [[ -x ./db_compact.sh ]]; then
  run_step "database migrations" cargo run --quiet --bin db_migrate
  run_step "database compaction" ./db_compact.sh
fi

# Skeleton files
[[ -f requirements.txt ]]         || echo "# placeholder" > requirements.txt
[[ -f README.md       ]]         || echo -e "# $APP_NAME\n\nBootstrap complete. Next steps:\n- Edit README\n- Push code\n" > README.md
[[ -f .pre-commit-config.yaml ]] || echo "# See https://pre-commit.com" > .pre-commit-config.yaml

# Optional builds
if [[ -f Makefile ]]; then
  if (( NATIVE_MONITOR == 1 )); then
    run_step "monitor (native)" env DETACH=1 make monitor --native-monitor
  else
    run_step "monitor" env DETACH=1 make monitor
  fi
fi
[[ -f CMakeLists.txt ]] && run_step "cmake build" bash -c 'mkdir -p build && cd build && cmake .. && make'
[[ -f justfile || -f Justfile ]] && run_step "just" just

# Python/Node deps (all pip/poetry only if not broken)
if (( BROKEN_PYTHON == 0 )); then
  if [[ -s requirements.txt ]]; then
    if ! run_step "pip install requirements" pip install -r requirements.txt; then
      cecho yellow "   → continuing without optional Python deps"
    fi
  fi
  if [[ -f pyproject.toml ]] && command -v poetry &>/dev/null; then
    run_step "poetry install" poetry install
  fi
  if [[ -f .pre-commit-config.yaml ]] && [[ "${CI:-}" == "" ]]; then
    run_step "pip install pre-commit" pip install pre-commit
    run_step "pre-commit install" pre-commit install
  fi
else
  skip_step "pip/poetry/pre-commit install (Python broken: sqlite3 missing)"
fi

if [[ -f package.json && -s package.json ]]; then
  if [[ -f pnpm-lock.yaml ]];  then run_step "pnpm install" pnpm install
  elif [[ -f yarn.lock    ]];  then run_step "yarn install" yarn install
  elif [[ -f package-lock.json ]]; then run_step "npm ci" npm ci || npm install
  else run_step "npm install" npm install
  fi
else
  cecho yellow "   → No valid package.json found or file is empty; skipping Node deps."
  SKIPPED_STEPS+=("npm install skipped (no valid package.json)")
fi
if ! node -e 'require("./package.json")' 2>/dev/null; then
  cecho red "   → package.json is STILL invalid after rewrite. Please fix it."
  FAILED_STEPS+=("package.json invalid even after auto-fix")
  FIX_COMMANDS+=("echo '{}' > package.json")
fi

# Docker check
if ! command -v docker &>/dev/null; then
  skip_step "Docker not detected (devnet/CI features skipped)"
fi


# direnv, pipx
if command -v direnv &>/dev/null && [[ -f .envrc ]]; then
  cecho cyan "   → direnv detected; run 'direnv allow' if needed."
fi
if command -v pipx &>/dev/null && [[ -f requirements.txt ]]; then
  cecho cyan "   → pipx detected; you may want to run 'pipx install -r requirements.txt'"
fi

# --------------------------------------------------------------------
# 11. Build and install the Rust Python native extension (via maturin)
# --------------------------------------------------------------------
# Only build if Python is not broken, maturin is installed, and Cargo.toml exists (i.e. this is a Rust/PyO3 project)
if (( BROKEN_PYTHON == 0 )) && command -v maturin &>/dev/null && [[ -f Cargo.toml ]]; then
  command -v patchelf &>/dev/null || run_step "pip install patchelf" pip install patchelf
  run_step "maturin develop --release (build Python native module)" maturin develop --release
else
  skip_step "maturin develop (no maturin, no Cargo.toml, or Python is broken)"
fi

# Misc checks, diagnostics, output
if [[ -f .env ]] && grep -q 'changeme' .env; then
  cecho yellow "⚠  Replace placeholder secrets in .env before production use!"
fi

cecho green "==> [$APP_NAME] bootstrap complete"
cecho cyan "   python demo.py now works without activation"
for exe in python cargo just docker cargo-nextest; do
  if command -v $exe &>/dev/null; then
    if [[ "$exe" == "python" ]]; then
      cecho blue "   $($exe -V 2>&1 | head -n1)"
    else
      cecho blue "   $($exe --version 2>&1 | head -n1)"
    fi
  else
    cecho yellow "   $exe not found"
  fi
done

if command -v cargo-make &>/dev/null; then
  cecho blue "   $(cargo make --version 2>&1 | head -n1)"
else
  cecho yellow "   cargo-make not found"
fi

if (( PY_SHIM == 1 )); then
  cecho yellow "   python shim active; add $(pwd)/bin to PATH or use python3"
fi

if (( ${#FAILED_STEPS[@]} )); then
  cecho red "\n⚠  Some steps failed:"
  for step in "${FAILED_STEPS[@]}"; do
    cecho yellow "   - $step"
  done
  cecho cyan "Fix the above issues (commands shown), then re-run bootstrap or the failing commands manually."
fi
if (( ${#SKIPPED_STEPS[@]} )); then
  cecho yellow "\n⚠  Some non-essential steps were skipped:"
  for step in "${SKIPPED_STEPS[@]}"; do
    cecho yellow "   - $step"
  done
fi
if (( ${#FIX_COMMANDS[@]} )); then
  cecho yellow "\nAuto-detected FIX COMMANDS (run these to repair common failures):"
  for step in "${FIX_COMMANDS[@]}"; do
    cecho blue "   $step"
  done
fi

if (( BROKEN_PYTHON == 1 )); then
  cecho red "\n************"
  cecho red "YOUR PYTHON ENV IS BROKEN (NO SQLITE3):"
  cecho yellow "To repair, run the following commands, then re-run bootstrap:"
  for step in "${FIX_COMMANDS[@]}"; do cecho blue "   $step"; done
  cecho red "************"
fi
rm -f "$PARTIAL_RUN_FLAG"
exit 0
