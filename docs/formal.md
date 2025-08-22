# Formal Verification

The project uses [F★](https://www.fstar-lang.org/) to state and check compute-market invariants.

## Running the Checks

```bash
make -C formal
```

The `formal/Makefile` calls `scripts/install_fstar.sh`, which detects your OS and
CPU architecture, downloads the matching release (default `v2025.08.07`),
verifies its SHA256 checksum, and caches the binaries under
`formal/.fstar/<version>/`. Override the pinned release with `FSTAR_VERSION` or
point `FSTAR_HOME` at an existing installation to skip the download.

It then builds:

- `Compute_market.fst` – basic well-formedness of offers.
- `Compute_market_invariants.fst` – models offer, job, and account balances
  and proves that total bonds are preserved across the offer → job →
  finalization transitions.

The targets emit `.checked` files when verification succeeds.
