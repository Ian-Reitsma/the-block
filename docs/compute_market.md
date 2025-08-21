# Compute Market Workloads

The compute market supports running real workloads on job slices. Each slice
contains input data that is executed by a workload runner and produces a proof
hash.

## Workload Formats

- **Transcode** – Accepts raw bytes representing media to be transcoded. For the
  reference implementation the bytes are hashed with BLAKE3 and the hash is
  returned as the slice proof.
- **Inference** – Accepts serialized model input bytes. The reference runner
  hashes the input with BLAKE3 and returns the hash as the proof.

Both formats are deterministic; identical inputs always yield the same hash. The
`WorkloadRunner` dispatches to the appropriate reference job based on the
`Workload` enum and returns the proof hash for inclusion in `SliceProof`.

## Courier Mode

Nodes can operate in a carry-to-earn mode by storing bundle receipts and
forwarding them when connectivity is restored. Use the CLI to manage receipts:

```bash
# store a receipt for bundle.bin from sender alice
node compute courier send bundle.bin alice
# forward all pending receipts
node compute courier flush
```

Receipts are persisted in `sled` until acknowledged and rewards are credited
when forwarded. Flushing streams entries directly from the underlying
database iterator, so memory usage remains constant even with large receipt
queues.
