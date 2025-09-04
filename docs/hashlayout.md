# Hash Layout and Genesis Seeding

Block hashes are derived by serialising header fields in a fixed order and
feeding them to BLAKE3.  The layout is implemented in
[`node/src/hashlayout.rs`](../node/src/hashlayout.rs) and used by
[`hash_genesis.rs`](../node/src/hash_genesis.rs) to compute the canonical genesis
hash at compile time.

## Header Field Order

`BlockEncoder::encode` appends the following fields in little‑endian form:

1. `index`
2. `prev` (hex string)
3. `timestamp`
4. `nonce`
5. `difficulty`
6. `coin_c`
7. `coin_i`
8. `storage_sub`
9. `read_sub`
10. `compute_sub`
11. `read_root`
12. `fee_checksum`
13. `state_root`
14. Each `l2_root`
15. Each `l2_size`
16. `vdf_commit`
17. `vdf_output`
18. `vdf_proof` length and bytes
19. Each transaction ID

The deterministic ordering ensures identical hashes across platforms and
language bindings.

## Genesis Hash Derivation

`hash_genesis.rs` builds a `BlockEncoder` with zeroed fields and calls
`const_hash()` to embed the genesis hash in the binary.  Pseudocode:

```text
encoder = BlockEncoder{
  index: 0,
  prev: ZERO_HASH,
  timestamp: 0,
  nonce: 0,
  difficulty: 8,
  coin_c: 0,
  coin_i: 0,
  storage_sub: 0,
  read_sub: 0,
  compute_sub: 0,
  read_root: [0;32],
  fee_checksum: ZERO_HASH,
  state_root: ZERO_HASH,
  tx_ids: [],
  l2_roots: [],
  l2_sizes: [],
  vdf_commit: [0;32],
  vdf_output: [0;32],
  vdf_proof: [],
}
GENESIS_HASH = blake3(encode(encoder))
```

## Versioning

Changes to field order or encoding require a `HASHLAYOUT_VERSION` bump and a
regeneration of `genesis_hash.txt` during build. Downstream tools (explorer,
wallet) must be updated to parse the new layout.  Older nodes reject blocks with
unknown versions to prevent silent forks.

## Tooling Impact

- **Explorer** – block parsers must mirror the new field order or they will
  compute incorrect hashes.
- **Wallets** – transaction inclusion proofs depend on the hash layout; bumping
  the version invalidates cached proofs.

## Tests

`node/tests/hashlayout.rs` (to be added) should assert that the compile‑time
`GENESIS_HASH` matches the runtime computation from `calculate_genesis_hash_runtime()`.
