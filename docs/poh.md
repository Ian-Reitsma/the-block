# Proof-of-History Tick Generator

The PoH subsystem produces a deterministic sequence of hashes that act as a
"clock" for consensus and receipt auditing. Each tick is the BLAKE3 hash of the
previous tick, forming a sequential chain that proves wall-clock progression
without relying on external time sources.

## Tick and Record Operations

The `Poh` struct maintains the current hash and tick counter:

```rust
use the_block::poh::Poh;
let mut poh = Poh::new(b"seed");
let h1 = poh.tick();        // advance one step
let h2 = poh.record(b"io"); // mix arbitrary data and advance
assert_eq!(poh.ticks(), 2);
```

`tick()` hashes the previous value, while `record()` folds arbitrary bytes into
the sequence before finalising the hash. Both methods increment the tick count.

## GPU Offload

When compiled with the `gpu` feature, `hash_step` delegates hashing to the
compute-market GPU runner. This maintains bit-for-bit determinism across CPU
and GPU targets while allowing operators with spare GPUs to accelerate PoH
production.

## Verification

Auditors recompute the hash chain starting from the agreed seed. Because each
step depends solely on the previous hash and optional data payload, any
malformed tick is immediately detectable. The chain can be truncated or
extended without ambiguity, enabling efficient batching of tick proofs.

## Security Considerations

- **Determinism** – the hash function and data ordering are fixed, guaranteeing
  identical results across platforms.
- **Fork Detection** – divergences in the hash chain reveal attempts to rewind
  or fast-forward time.
- **Seed Selection** – seeds should be unpredictable; genesis blocks use
  governance-approved entropy and are documented in `docs/genesis_history.md`.

See `node/src/poh.rs` for the implementation and `node/tests/poh.rs` for round
trip tests.
