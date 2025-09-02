# Storage Threat Model

Attack surfaces and mitigations for on-chain blob storage.

## Threats & Mitigations

- **Blob-spam floods** – mitigated by per-epoch L2 byte caps and per-sender quotas.
- **Shard withholding** – data availability sampling and long-tail audits slash non-responsive miners.
- **Rent evasion** – each blob locks `rent_rate_ct_per_byte * size` in escrow; 90 % refunded on delete/expiry, 10 % burned.
- **Permanent state bloat** – TTL defaults and refundable rent incentivise pruning.

See [../storage_pipeline.md](../storage_pipeline.md) for pipeline internals.
