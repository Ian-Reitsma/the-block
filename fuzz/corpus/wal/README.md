# WAL Seed Corpus
> **Review (2025-09-25):** Synced WAL Seed Corpus guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Curated seeds for `wal_fuzz` live here. To promote a new seed:

1. Run `scripts/extract_wal_seeds.sh fuzz/wal` to list interesting cases.
2. Copy the desired artifact into this directory.
3. Commit the file so it becomes part of the versioned corpus.

Each file name should retain the original fuzz artifact hash and the RNG seed.
