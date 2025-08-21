# Formal Verification

The project uses [F★](https://www.fstar-lang.org/) to state and check compute-market invariants.

## Running the Checks

```bash
make -C formal
```

The `formal/Makefile` will automatically download a pinned F★ release to
`formal/.fstar/` if `fstar.exe` is not already installed. Override the
version by setting `FSTAR_VERSION`.

It then builds:

- `Compute_market.fst` – basic well-formedness of offers.
- `Compute_market_invariants.fst` – models offer, job, and account balances
  and proves that total bonds are preserved across the offer → job →
  finalization transitions.

The targets emit `.checked` files when verification succeeds.
