# Risk Memo

- **Fuzz Harness**: `TB_MIN_FEE_PER_BYTE=0 cargo test --test test_chain --features fuzzy`
  ran 24 cases (cross-thread, randomized fees/nonces) with no panics or
  invariant violations. See `fuzz.log` for detailed output.
- **Schema Migration**: `cargo test test_schema_upgrade_compatibility`
  exercised v1–v3 disk images upgrading to schema v4 and reported no errors.
  See `migration.log` for transcript.
- **Residual Risk**: Fuzz iterations remain limited to ~10k and cover only
  in-memory execution; long-run network fuzzing and durable storage backends
  remain unvalidated. A second senior engineer must review concurrency and
  panic-recovery logic prior to merge or testnet deployment.
