# Storage Threat Model
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

## Rent Escrow
- Blobs pay `rent_rate_ct_per_byte` on admission, locked under `rent_escrow.db`.
- Deletion or expiry refunds 90 % and burns 10 %, tracked by `rent_escrow_refunded_ct_total` and `rent_escrow_burned_ct_total`.
- Governance can adjust `rent_rate_ct_per_byte` to throttle usage.

Example: uploading a 5 MB image at a rent rate of 0.05 µCT/B locks 250 CT in the
escrow database. Deleting the blob later returns 225 CT to the uploader and burns
25 CT. The burn both discourages indefinite storage and provides a counterbalance
to inflation. Operators should monitor `rent_escrow_locked_ct_total` to ensure
escrow size tracks expected usage.

## L2 Byte Caps
- Per-epoch byte caps prevent unbounded growth; exceeding caps rejects uploads.

The default cap `storage.l2_cap_bytes_per_epoch` is 32 MB per shard per epoch.
Clients attempting to exceed the cap receive `ERR_RENT_ESCROW_INSUFFICIENT`, and
the transaction is dropped before touching disk. Governance can raise caps as
hardware capacity grows, but lowering them requires a two-epoch notice to avoid
surprise rejections.

## Shard Auditing
- Nodes sample shards and compare Merkle roots to detect tampering.

Sampling frequency uses a VRF so adversaries cannot predict which shard will be
requested. Failing to provide a requested shard within the challenge window
results in an immediate CT burn proportional to the blob size and marks the node
for follow-up audits.

## Long-Tail Audits
- Randomized deep checks ensure rarely accessed data remains available.

Audits may reach back hundreds of epochs. Nodes that prune data prematurely risk
losing their stake and any accumulated rent deposits. Audit requests include the
blob commitment and expected expiry height so reconstructing old data is
cryptographically provable.

## Slashing
- Missing or corrupt data triggers CT burns via slashing penalties.

Slashing emits `slashing_burned_ct_total` telemetry, enabling community auditors
to correlate penalties with offending accounts. Repeat offenders can be
blacklisted from future storage roles through governance votes.

### Cross-Cutting
- Subsidy multipliers (`beta/gamma/kappa/lambda`) retune each epoch; see [economics](../economics.md#epoch-retuning-formula).
- Kill switch parameter can globally downscale subsidies when `kill_switch_subsidy_reduction` is set.
- Bonded service roles and salted IP hashing defend against Sybil attacks.
- Operators monitor `rent_escrow_locked_ct_total` and multiplier gauges in telemetry.
- If `rent_escrow_locked_ct_total` grows faster than expected network adoption,
investigate for spam uploads or misconfigured rent rates; governance may tighten
caps or raise the rent rate to restore equilibrium.