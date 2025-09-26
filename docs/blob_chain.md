# Blob Scheduler and Multi-Layer Root Anchoring
> **Review (2025-09-25):** Synced Blob Scheduler and Multi-Layer Root Anchoring guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`BlobScheduler` coordinates when pending blob roots are aggregated and
anchored on-chain. The scheduler tracks two independent queues:

- **L2 queue** – roots for blobs up to 4 GiB, flushed every 4 seconds.
- **L3 queue** – roots for blobs larger than 4 GiB, flushed every 16 seconds.

## Scheduling Semantics

Roots enter the scheduler via [`push`](../node/src/blob_chain.rs#L22-L28), which
drops the root into either the L2 or L3 queue based on the caller’s `is_l3`
flag. Every time [`pop_l2_ready`](../node/src/blob_chain.rs#L31-L39) or
[`pop_l3_ready`](../node/src/blob_chain.rs#L41-L49) is invoked, the scheduler
checks whether the cadence for that layer has elapsed. When the timer expires
it drains the corresponding queue and returns all collected roots, resetting the
last-seen timestamp. Calls made before the interval lapses return an empty
vector, allowing polling without spurious anchors.

The scheduler exposes no internal concurrency; callers are expected to hold a
mutable reference during enqueue/dequeue operations. The default constructor
[`BlobScheduler::new`](../node/src/blob_chain.rs#L12-L20) initializes empty
queues with zeroed timers, and the [`Default`](../node/src/blob_chain.rs#L52-L53)
trait forwards to it for ergonomic struct instantiation.

## Integration Points

`Blockchain` embeds the scheduler as `blob_scheduler` and initializes it in the
node’s default state so storage pipelines can begin enqueuing roots immediately
[`node/src/lib.rs#L687-L693`](../node/src/lib.rs#L687-L693) and
[`node/src/lib.rs#L798-L801`](../node/src/lib.rs#L798-L801). Pipelines call
`push` for each finalized blob and periodically poll `pop_l2_ready` and
`pop_l3_ready` to obtain batches ready for Merkle aggregation and anchoring.

In typical deployments the L2 cadence captures rapid small uploads while L3
serves bulk archival writes. Operators should ensure their anchoring task polls
both queues at least once per cadence and monitor for excessive queue depths as
a sign of upload backlogs. Telemetry counters for queue length and cadence
misses are planned.

## Example

```rust
use the_block::blob_chain::BlobScheduler;

let mut sched = BlobScheduler::new();
// Enqueue a 2 GiB blob root (L2)
sched.push([0u8; 32], false);
// Enqueue a 10 GiB blob root (L3)
sched.push([1u8; 32], true);

// Later, poll for ready batches
let l2 = sched.pop_l2_ready();
let l3 = sched.pop_l3_ready();
```

This pattern allows storage clients to accumulate roots without blocking while
ensuring anchoring frequency stays bounded.
