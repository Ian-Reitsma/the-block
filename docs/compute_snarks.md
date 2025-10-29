# Compute Market SNARK Guidelines
> **Review (2025-10-01):** Noted the in-house Groth16 backend powering compute-market proofs.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Providers may supply Groth16/Plonk proofs for compute workloads. Proofs are
constructed over small WASM tasks compiled into circuits. The `compute_market`
crate exposes `snark::compile_wasm`, `snark::prove`, and `snark::verify` helpers
backed by the first-party BN254 Groth16 engine.

## Ad-selection manifest and verifier

Advertising selection proofs re-use the same Groth16 backend via the
`zkp::selection` helper. Circuits still ship inside the embedded
`crates/zkp/resources/selection_manifest.json`, but the loader now installs those
descriptors into a runtime registry guarded by `foundation_lazy::Lazy`. Governance
or integration suites can push updates with `install_selection_manifest`, which
accepts a JSON manifest parsed entirely through
`foundation_serialization::json::Value`. The registry rejects descriptor sets that
regress a circuit’s revision and only increments the manifest epoch when the new
set is monotonic, so wallets cannot downgrade published circuits. Callers inspect
the active revision/epoch/tag via `selection_manifest_version()` and
`selection_manifest_tag()` and fetch the current descriptors with
`selection_circuit_summaries()` without touching third-party parsers.

Each entry fixes:

- `revision`: circuit revision number expected in proof envelopes.
- `expected_version`: logical envelope version (`SelectionProofEnvelope::version`).
- `min_proof_len`: minimum byte length for the SNARK blob.
- `transcript_domain_separator`: domain separator mixed into the transcript
  digest (defaults to the circuit id when omitted).
- `expected_witness_commitments`: number of 32-byte witness commitments wallets
  must supply (optional).
- `expected_protocol`: canonical protocol string (lowercased before comparison).

`zkp::selection::verify_selection_proof` cross-checks manifest data against the
provided proof, recomputes the transcript digest via BLAKE3, enforces the
composite resource floor semantics encoded in the public inputs
(`winner_quality_bid_usd_micros`, `runner_up_quality_bid_usd_micros`,
`resource_floor_usd_micros`, `clearing_price_usd_micros`), and returns a
`SelectionProofVerification` structure containing the normalised metadata. Ad
market receipts consume this structure via `SelectionProofMetadata`, recording the
manifest epoch/tag, transcript domain separator, expected witness commitments,
protocol casing, and public inputs alongside the composite resource floor. During
attestation the committee transcript set is normalised against the proof digest
and stamped with the active manifest epoch so settlement records prove both
SNARK honesty and committee co-signature without external tooling.

1. Compile the workload WASM into a circuit representation:
   ```rust
   let circuit = the_block::compute_market::snark::compile_wasm(wasm_bytes);
   ```
2. Execute the workload and produce an output hash.
3. Create a proof:
   ```rust
   let proof = the_block::compute_market::snark::prove(&circuit, &output_hash);
   ```
4. Submit the proof inside an `ExecutionReceipt`. The scheduler verifies the
   proof before crediting payment. Failed verifications increment the
    `snark_fail_total` telemetry counter while successes increment
    `snark_verifications_total`.

Workloads that omit SNARK proofs may leave the receipt's proof field empty;
the scheduler then credits payment without verification.

These primitives are intentionally small to keep provider integration simple.
More advanced circuits can be built by substituting the `compile_wasm` step with
alternative circuit compilers while still targeting the shipped Groth16 backend.
