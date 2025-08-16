# Service Badge Tracker

Nodes earn a *service badge* for sustained uptime. Each badge represents
approximately 90 days of near-perfect availability and is intended for future
governance voting.

The tracker records whether the node was up for each 600-block epoch and its
latency sample. When 90 epochs have been recorded and uptime exceeds 99%, a
badge is minted.

```rust
use the_block::ServiceBadgeTracker;
let mut tracker = ServiceBadgeTracker::new();
for _ in 0..90 {
    tracker.record_epoch(true, std::time::Duration::from_millis(0));
}
tracker.check_badges();
assert!(tracker.has_badge());
```

`Blockchain::mine_block` automatically records epochs every 600 blocks and
updates the badge tracker. The current node's badge status can be queried with
`Blockchain::has_badge()`.
