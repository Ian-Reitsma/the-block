# Formal Verification
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The project uses [F★](https://www.fstar-lang.org/) to state and check compute-market invariants.

## Running the Checks

```bash
make -C formal
```

The `formal/Makefile` calls `scripts/install_fstar.sh`, which detects your OS and
CPU architecture, downloads the matching release (default `v2025.08.07`),
verifies its SHA256 checksum, and caches the binaries under
`formal/.fstar/<version>/`. The installer exports `FSTAR_HOME` for downstream
tools so the path does not need to be rediscovered on subsequent runs. Override
the pinned release with `FSTAR_VERSION=<tag>` or point `FSTAR_HOME` at an
existing installation to skip the download entirely.

Examples:

```bash
# Linux/macOS
FSTAR_VERSION=v2025.08.07 make -C formal

# Windows PowerShell
set FSTAR_VERSION v2025.08.07
make -C formal

# Reuse an existing install
FSTAR_HOME=$HOME/tools/fstar make -C formal
```

It then builds:

- `Compute_market.fst` – basic well-formedness of offers.
- `Compute_market_invariants.fst` – models offer, job, and account balances
  and proves that total bonds are preserved across the offer → job →
  finalization transitions.

The targets emit `.checked` files when verification succeeds.
A regression script (`scripts/test_install_fstar.sh`) verifies that the installer fails gracefully on unknown versions.

See [AGENTS.md](../AGENTS.md#17-agent-playbooks--consolidated) for contributor guidelines and installer flags.

## Troubleshooting

- **Checksum mismatch** – delete `formal/.fstar/<version>` and rerun; ensure the
  download completed and your network proxy is not tampering with the archive.
- **`curl` missing** – install `curl` via your package manager (`apt install
  curl`, `brew install curl`, or `winget install curl`).