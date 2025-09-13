# Compute SNARKs

This document outlines how compute providers can generate SNARK proofs for
workloads submitted through the compute marketplace. Tasks can optionally
compile a small WASM program into a circuit and submit execution receipts with
a corresponding proof.

1. Compile the WASM payload into a circuit using `cli snark compile`.
2. Execute the workload off-chain and produce an output hash.
3. Generate a proof bound to the workload and output.
4. Submit the proof alongside the execution receipt. The scheduler verifies the
   proof before crediting payment.

Providers may fall back to non-SNARK workloads by omitting the proof field.
Invalid proofs will be rejected and the payout forfeited.
