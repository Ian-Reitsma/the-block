#!/usr/bin/env bash
# ====================================================================
# the-block ▸ universal bootstrap (Linux • macOS • WSL2 • Codex/CI/empty-repo safe)
# ====================================================================
set -Eeuo pipefail

PARTIAL_RUN_FLAG=".bootstrap_partial"
touch "$PARTIAL_RUN_FLAG"

cleanup() {
  if [[ -f "$PARTIAL_RUN_FLAG" ]]; then
    cecho red "[ABORTED] Cleaning up partial bootstrap/build artifacts..."
      [[ -d build ]] && rm -rf build && cecho yellow "  → Removed ./build directory"
      [[ -d dist ]] && rm -rf dist && cecho yellow "  → Removed ./dist directory"
      [[ -d __pycache__ ]] && rm -rf __pycache__ && cecho yellow "  → Removed Python __pycache__"
      [[ -d .pytest_cache ]] && rm -rf .pytest_cache && cecho yellow "  → Removed .pytest_cache"
      [[ -d node_modules && ! -s package.json ]] && rm -rf node_modules && cecho yellow "  → Removed node_modules (no package.json found)"
      [[ -d .mypy_cache ]] && rm -rf .mypy_cache && cecho yellow "  → Removed .mypy_cache"
      [[ -f CMakeCache.txt ]] && rm -f CMakeCache.txt && cecho yellow "  → Removed CMakeCache.txt"
      [[ -d CMakeFiles ]] && rm -rf CMakeFiles && cecho yellow "  → Removed CMakeFiles directory"
      [[ -d pip-wheel-metadata ]] && rm -rf pip-wheel-metadata && cecho yellow "  → Removed pip wheel metadata"
      [[ -d .tox ]] && rm -rf .tox && cecho yellow "  → Removed .tox environment"
      [[ -f poetry.lock && ! -f pyproject.toml ]] && rm -f poetry.lock && cecho yellow "  → Removed poetry.lock (orphaned)"
      [[ -f package-lock.json && ! -f package.json ]] && rm -f package-lock.json && cecho yellow "  → Removed package-lock.json (orphaned)"
      [[ -f pnpm-lock.yaml && ! -f package.json ]] && rm -f pnpm-lock.yaml && cecho yellow "  → Removed pnpm-lock.yaml (orphaned)"
      [[ -f yarn.lock && ! -f package.json ]] && rm -f yarn.lock && cecho yellow "  → Removed yarn.lock (orphaned)"
      [[ -f bootstrap.log ]] && rm -f bootstrap.log && cecho yellow "  → Removed bootstrap.log"
      if [[ -n "${CI:-}" && -n "$(command -v docker)" ]]; then
        cecho yellow "Cleaning up Docker containers/images from interrupted bootstrap..."
        docker ps -aq --filter "label=the-block-bootstrap" | xargs -r docker rm -f
        docker images -q --filter "label=the-block-bootstrap" | xargs -r docker rmi -f
      fi
    cecho red "[CLEANUP DONE] Exiting due to error/interruption."
  fi
}

trap cleanup SIGINT SIGTERM ERR

# ---- Color Echo Helper ----
cecho() {
  local color="$1"; shift
  case "$color" in
    red) color="31";;
    green) color="32";;
    yellow) color="33";;
    blue) color="34";;
    cyan) color="36";;
    *) color="0";;
  esac
  echo -e "\033[1;${color}m$*\033[0m"
}

if [[ ! "$SHELL" =~ (bash|zsh)$ ]]; then
  cecho yellow "[WARN] Detected shell: $SHELL. This script is tested on Bash/Zsh. If you use Fish or tcsh, odd errors may occur."
fi

APP_NAME="the-block"
REQUIRED_PYTHON="3.12.3"
REQUIRED_NODE="20"
NVM_VERSION="0.39.7"

cecho cyan "==> [$APP_NAME] Universal Bootstrap"

# --------------------------------------------------------------------
# 0. OS / arch / shell probe
# --------------------------------------------------------------------
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
IS_WSL=false
IS_CI="${CI:-false}"
[[ -f /proc/version ]] && grep -qi microsoft /proc/version && IS_WSL=true && cecho green "   → Running inside WSL2."
[[ -n "${CODESPACES:-}" || -n "${GITPOD_WORKSPACE_ID:-}" || -n "${OPENAI_CLI_ENV:-}" ]] && IS_CI=true

# --------------------------------------------------------------------
# 1. .env example sync (skip if missing)
# --------------------------------------------------------------------
if [[ -f .env.example ]]; then
  [[ -f .env ]] || cp .env.example .env
  cecho blue "   → Verifying env keys…"
  missing=$(comm -23 \
    <(grep -v '^#' .env.example | grep -o '^[A-Za-z_][A-Za-z0-9_]*' | sort) \
    <(grep -v '^#' .env 2>/dev/null | grep -o '^[A-Za-z_][A-Za-z0-9_]*' | sort)) || true
  if [[ -n "${missing}" ]]; then
    cecho yellow "      Adding missing keys from .env.example:"
    for k in $missing; do
      grep "^$k=" .env.example >> .env
      cecho green "        + $k"
    done
  fi
else
  cecho yellow "   → No .env.example found, skipping env sync."
fi

# --------------------------------------------------------------------
# 2. System build deps (warn but do not fail)
# --------------------------------------------------------------------
install_pkgs() {
  if command -v apt-get &>/dev/null; then
    timeout 60s sudo apt-get update || cecho red "[ERROR] apt-get update timed out after 60s; continuing anyway."
    sudo apt-get install -y build-essential zlib1g-dev libffi-dev libssl-dev \
      libbz2-dev libreadline-dev libsqlite3-dev curl git jq lsof pkg-config \
      python3 python3-venv python3-pip cmake make || true
  elif command -v dnf &>/dev/null; then
    sudo dnf install -y nodejs npm
    sudo dnf install -y gcc gcc-c++ make openssl-devel zlib-devel readline-devel \
      curl git jq lsof pkg-config cmake
    sudo dnf install -y python3.12 python3.12-venv python3.12-pip python3.12-libs python3-virtualenv python3-pip
    sudo dnf install -y sqlite-devel
  elif [[ "$OS" == "darwin" ]]; then
    if ! command -v brew &>/dev/null; then
      cecho yellow "Homebrew not found — installing…"
      /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
      eval "$(/opt/homebrew/bin/brew shellenv)" || true
    fi
    brew update || true
    brew install git jq lsof pkg-config openssl@3 python@3.12 cmake make || true
  else
    cecho red "   → No supported system package manager; skipping system deps."
  fi
}
install_pkgs

# --------------------------------------------------------------------
# 3. Python 3.12.x, auto-pyenv/conda fallback if not found
# --------------------------------------------------------------------
PY_BIN=""
if command -v python3.12 &>/dev/null; then
  PY_BIN="$(command -v python3.12)"
elif python3 --version 2>/dev/null | grep -q '3\.12'; then
  PY_BIN="$(command -v python3)"
elif command -v conda &>/dev/null && conda info --envs | grep -q '3\.12'; then
  cecho cyan "   → Using Python 3.12 from Conda environment."
  PY_BIN="$(conda run which python)"
else
  if ! command -v pyenv &>/dev/null; then
    cecho yellow "   → Installing pyenv (user mode)…"
    curl https://pyenv.run | bash
    export PATH="$HOME/.pyenv/bin:$PATH"
    eval "$(pyenv init -)"
    eval "$(pyenv virtualenv-init -)"
  else
    export PATH="$HOME/.pyenv/bin:$PATH"
    eval "$(pyenv init -)"
    eval "$(pyenv virtualenv-init -)"
  fi
  if ! pyenv versions --bare | grep -qx "$REQUIRED_PYTHON"; then
    cecho blue "   → Building Python $REQUIRED_PYTHON with pyenv (may take a few min)…"
    pyenv install -s "$REQUIRED_PYTHON"
  fi
  pyenv local "$REQUIRED_PYTHON"
  PY_BIN="$(pyenv which python)"
fi

if [[ ! -d .venv ]]; then
  cecho cyan "   → Creating venv with Python $REQUIRED_PYTHON"
  "$PY_BIN" -m venv .venv
fi
source .venv/bin/activate
if command -v conda &>/dev/null && conda info --envs 2>/dev/null | grep -q '*'; then
  ACTIVE_CONDA=$(conda info --envs | awk '/\*/ {print $1}')
  if [[ -n "$ACTIVE_CONDA" && "$ACTIVE_CONDA" != "base" ]]; then
    cecho yellow "[WARN] You are in Conda env: $ACTIVE_CONDA. To use .venv, run: 'conda deactivate' first."
  fi
fi
if command -v pyenv &>/dev/null && [[ "$(pyenv version-name)" != "$REQUIRED_PYTHON" ]]; then
  cecho yellow "[WARN] pyenv is active but not set to $REQUIRED_PYTHON. Run: 'pyenv local $REQUIRED_PYTHON'."
fi

python -m pip install --upgrade pip setuptools wheel

# --------------------------------------------------------------------
# 4. Node/NVM (+Yarn/pnpm support)
# --------------------------------------------------------------------
export NVM_DIR="$HOME/.nvm"
if [[ ! -s "$NVM_DIR/nvm.sh" ]]; then
  cecho blue "   → Installing NVM $NVM_VERSION…"
  curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v$NVM_VERSION/install.sh | bash
fi
# shellcheck source=/dev/null
source "$NVM_DIR/nvm.sh"
if ! command -v node &>/dev/null || [[ "$(node -v)" != v$REQUIRED_NODE* ]]; then
  cecho cyan "   → Installing Node.js $REQUIRED_NODE via nvm…"
  nvm install $REQUIRED_NODE
  nvm alias default $REQUIRED_NODE
  nvm use $REQUIRED_NODE
fi
if ! command -v yarn &>/dev/null; then
  cecho cyan "   → Installing yarn globally…"
  npm install -g yarn || true
fi
if ! command -v pnpm &>/dev/null; then
  cecho cyan "   → Installing pnpm globally…"
  npm install -g pnpm || true
fi

# --------------------------------------------------------------------
# 5. Rust tool-chain (+ just, cargo-make, maturin)
# --------------------------------------------------------------------
if [[ ! -d .venv ]]; then
  cecho cyan "   → Creating venv with Python $REQUIRED_PYTHON"
  "$PY_BIN" -m venv .venv
fi
source .venv/bin/activate

python -m pip install --upgrade pip setuptools wheel

# Install maturin from pip to avoid cargo build issues
pip install --upgrade maturin

# --- Rust toolchain, just, cargo-make installs ---
if ! command -v cargo &>/dev/null; then
  cecho cyan "   → Installing Rust…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi

if ! command -v just &>/dev/null; then
  cecho cyan "   → Installing just (Rust task runner)…"
  cargo install just || true
fi

if ! command -v cargo-make &>/dev/null; then
  cecho cyan "   → Installing cargo-make (Rust build runner)…"
  cargo install cargo-make || true
fi


[[ -f requirements.txt ]] || echo "# placeholder" > requirements.txt
[[ -f README.md ]] || echo -e "# $APP_NAME\n\nBootstrap complete. Next steps:\n- Edit README\n- Push code\n" > README.md
[[ -f package.json ]] || echo '{ "name": "the-block", "version": "0.1.0" }' > package.json
[[ -f .pre-commit-config.yaml ]] || echo "# See https://pre-commit.com" > .pre-commit-config.yaml

# --------------------------------------------------------------------
# 6. Makefile/CMake/justfile (optional, best-effort build)
# --------------------------------------------------------------------
if [[ -f Makefile ]]; then
  cecho blue "   → Detected Makefile, running make (if present)…"
  make || cecho yellow "      (make failed or no default target, skipping)"
fi
if [[ -f CMakeLists.txt ]]; then
  cecho blue "   → Detected CMake project, running cmake…"
  mkdir -p build && cd build && cmake .. && make && cd .. || cecho yellow "      (cmake failed, skipping)"
fi
if [[ -f justfile || -f Justfile ]]; then
  cecho blue "   → Detected justfile, running just (if present)…"
  just || cecho yellow "      (just failed or no default target, skipping)"
fi

# --------------------------------------------------------------------
# 7. Python/Node deps (skip if missing)
# --------------------------------------------------------------------
if [[ -f requirements.txt ]]; then
  cecho blue "   → pip install -r requirements.txt"
  pip install -r requirements.txt
else
  cecho yellow "   → No requirements.txt found, skipping Python deps."
fi
if [[ -f pyproject.toml ]] && command -v poetry &>/dev/null; then
  cecho blue "   → poetry detected, running poetry install"
  poetry install || true
fi
if [[ -f package.json ]]; then
  if [[ -f pnpm-lock.yaml ]]; then
    cecho green "Using pnpm (pnpm-lock.yaml present)"
    pnpm install || true
  elif [[ -f yarn.lock ]]; then
    cecho green "Using yarn (yarn.lock present)"
    yarn install || true
  elif [[ -f package-lock.json ]]; then
    cecho green "Using npm ci (package-lock.json present)"
    npm ci || npm install
  else
    cecho yellow "No lockfile detected; running npm install (not reproducible!)"
    npm install || true
  fi
else
  cecho yellow "   → No package.json found, skipping Node deps."
fi

# --------------------------------------------------------------------
# 8. Docker (warn but don’t fail)
# --------------------------------------------------------------------
if ! command -v docker &>/dev/null; then
  cecho yellow "⚠  Docker not detected — devnet/CI features will be disabled."
fi

# --------------------------------------------------------------------
# 9. Pre-commit (skip in CI/empty), direnv, pipx
# --------------------------------------------------------------------
if [[ -f .pre-commit-config.yaml ]] && [[ "$IS_CI" == "false" ]]; then
  cecho blue "   → Installing pre-commit hooks"
  pip install pre-commit
  pre-commit install
fi
if command -v direnv &>/dev/null && [[ -f .envrc ]]; then
  cecho cyan "   → direnv detected; run 'direnv allow' if needed."
fi
if command -v pipx &>/dev/null && [[ -f requirements.txt ]]; then
  cecho cyan "   → pipx detected; you may want to run 'pipx install -r requirements.txt'"
fi

# --------------------------------------------------------------------
# 10. Misc: secrets, diagnostics, activation
# --------------------------------------------------------------------
if [[ -f .env ]] && grep -q 'changeme' .env; then
  cecho yellow "⚠  Replace placeholder secrets in .env before production use!"
fi

# Diagnostic summary
cecho green "==> [$APP_NAME] bootstrap complete"
cecho cyan "   Activate venv:   source .venv/bin/activate"
command -v node   &>/dev/null && cecho green "   Node:   $(node -v)"
command -v python &>/dev/null && cecho green "   Python: $(python -V)"
command -v rustc  &>/dev/null && cecho green "   Rust:   $(rustc --version)"
command -v cargo  &>/dev/null && cecho green "   Cargo:  $(cargo --version)"
command -v conda  &>/dev/null && cecho green "   Conda:  $(conda --version)"
command -v just   &>/dev/null && cecho green "   Just:   $(just --version)"
command -v docker &>/dev/null && cecho green "   Docker: $(docker --version)"
command -v yarn   &>/dev/null && cecho green "   Yarn:   $(yarn -v)"
command -v pnpm   &>/dev/null && cecho green "   pnpm:   $(pnpm -v)"

rm -f "$PARTIAL_RUN_FLAG"
exit 0
