# Service Badge Tracker

Nodes earn a *service badge* for sustained uptime. Each badge represents
approximately 90 days of near-perfect availability and is intended for future
governance voting.

The tracker records a heartbeat proof for each 600-block epoch along with a
latency sample. When 90 epochs have been recorded and uptime exceeds 99%, a
badge is minted. If uptime later falls below 95% the badge is revoked.

```rust
use the_block::ServiceBadgeTracker;
let mut tracker = ServiceBadgeTracker::new();
for _ in 0..90 {
    tracker.record_epoch(true, std::time::Duration::from_millis(0));
}
assert!(tracker.has_badge());
for _ in 0..90 {
    tracker.record_epoch(false, std::time::Duration::from_millis(0));
}
assert!(!tracker.has_badge());
```

`Blockchain::mine_block` automatically records epochs every 600 blocks and
updates the badge tracker. The current node's badge status can be queried with
`Blockchain::has_badge()`.

## HTTP Status Endpoint

Nodes expose `/badge/status` on the RPC port for external monitoring. The
endpoint returns a JSON object:

```json
{"active": true, "last_mint": 1700000000, "last_burn": null}
```

`active` indicates whether a badge is currently minted. `last_mint` and
`last_burn` expose UNIX timestamps of the most recent transitions, allowing
external monitors to track heartbeat cadence. Prometheus gauges
`badge_active` and `badge_last_change_seconds` surface the same information for
scrapes.
