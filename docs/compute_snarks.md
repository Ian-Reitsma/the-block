# Compute Market SNARK Guidelines
> **Review (2025-10-01):** Noted the in-house Groth16 backend powering compute-market proofs.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Providers may supply Groth16/Plonk proofs for compute workloads. Proofs are
constructed over small WASM tasks compiled into circuits. The `compute_market`
crate exposes `snark::compile_wasm`, `snark::prove`, and `snark::verify` helpers
backed by the first-party BN254 Groth16 engine.

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
