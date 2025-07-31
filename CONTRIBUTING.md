# Contributing

Thank you for helping improve The‑Block. Read `AGENTS.md` for the full developer handbook.

## Balance & Nonce Changes

Any pull request that touches account balance logic or nonce handling **must**:

1. Include a property test demonstrating that pending balances and nonces remain
   consistent after the change.
2. Provide migration notes in `docs/schema_migrations/` if the on‑disk schema is
   affected.
3. Update the diagrams in `docs/ledger_invariants.md` when state flows change.
4. Provide a Signed-off-by line in each commit message (Developer Certificate of Origin).
5. Update `formal/nonce_pending.fst` and attach the new SMT proof log.

Patches that do not satisfy these points will be rejected during review.

All code must retain at least **95% line** and **90% branch** coverage across
unit, property, and loom tests.  Coverage badges update automatically in the
README.

## PR Checklist

- [ ] property test updated
- [ ] migration script added if needed
- [ ] diagrams refreshed
- [ ] loom coverage ID
- [ ] Signed-off-by present
- [ ] F★ model proof attached
- [ ] coverage ≥95% lines and ≥90% branches
