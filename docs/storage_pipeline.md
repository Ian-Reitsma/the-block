# Storage Pipeline

The storage client splits objects into encrypted chunks before handing them to
providers. To keep uploads responsive across varied links, the pipeline adjusts
its chunk size on a per-provider basis:

- Allowed sizes: 256 KiB, 512 KiB, 1 MiB, 2 MiB, 4 MiB
- Target chunk time: ~3 s
- Per-chunk throughput, RTT, and loss are folded into an EWMA profile stored in
  `provider_profiles/{node_id}`.
- The preferred chunk size only changes after at least three stable chunks and
  shifts by one ladder step at a time. High loss (>2 %) or RTT (>200 ms) forces
  a downgrade; exceptionally clean links (<0.2 % loss, RTT <80 ms) allow
  upgrades.

Metrics exported via the telemetry feature include:

- `storage_chunk_size_bytes`
- `storage_put_chunk_seconds`
- `storage_provider_rtt_ms`
- `storage_provider_loss_rate`
- `storage_initial_chunk_size`
- `storage_final_chunk_size`
- `storage_put_eta_seconds`

Profiles persist across restarts so subsequent uploads reuse the last known
chunk size.
