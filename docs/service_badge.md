# Service Badge Tracker
> **Review (2025-09-25):** Synced Service Badge Tracker guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The service badge incentivizes long‑lived, responsive nodes. Operators earn a
badge after demonstrating 90 consecutive epochs of high availability; losing
availability revokes the badge until uptime is re‑established.

## Epoch Accounting

- **Epoch length** – 600 blocks.
- **Heartbeat** – each epoch records a boolean `up` flag and a latency sample.
- **Minting** – after 90 epochs (≈90 days) with `up` ≥99 %, the tracker mints a
  badge and records `last_mint` timestamp.
- **Revocation** – if the rolling window falls below 95 % uptime, the badge is
  burned and `last_burn` is updated.

```rust
use the_block::ServiceBadgeTracker;
let mut t = ServiceBadgeTracker::new();
for _ in 0..90 { t.record_epoch("node", true, std::time::Duration::from_millis(50)); }
assert!(t.has_badge());
```

Epochs are recorded automatically from `Blockchain::mine_block`, but external
systems may call `record_epoch` with a provider identifier for test harnesses.

## Provider badge registry

The tracker now records the last provider ID passed to `record_epoch` and
automatically mirrors badge state into a global registry. When a badge is
minted, `set_physical_presence(provider, true)` registers the provider under the
`physical_presence` flag so downstream consumers (gateway matching, metrics, and
governance audits) can query `provider_badges(provider_id)` for the active badge
set. Revocations call `revoke_physical_presence`, ensuring registry state stays
in sync with the tracker. Test harnesses and simulation code can inject or clear
badges directly with the helper functions:

```rust
use the_block::service_badge;
service_badge::set_physical_presence("gateway-nyc-01", true);
assert!(service_badge::provider_badges("gateway-nyc-01").contains(&"physical_presence".to_string()));
service_badge::clear_badges();
```

Future badge types should add explicit helpers and registry wiring alongside
their tracker logic so RPC/CLI surfaces can expose the augmented context without
polling sled directly.

## Telemetry & RPC

- Metrics: `badge_active`, `badge_last_change_seconds`, and
  `badge_latency_ms{quantile}` are exported via the runtime telemetry registry.
- RPC: `/badge/status` returns `{ "active": bool, "last_mint": u64,
  "last_burn": Option<u64> }`.
- CLI: `tb-cli badge status` queries the RPC endpoint and prints a human‑readable
  report.

## Governance & Persistence

Badges are stored in node state and are expected to feed future governance
weighting. Persistence ensures restarts do not reset progress; checkpoints are
included in snapshots.

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `badge_active` absent from metrics | Ensure telemetry is enabled and the runtime registry snapshot is being collected. |
| Progress stalls | Verify blocks are mined; badge tracking only advances every 600 blocks. |
| Unexpected revocation | Check for gaps in heartbeat logs or latency spikes above SLA. |

See `node/src/service_badge.rs` and `node/tests/service_badge.rs` for
implementation details and unit tests.
