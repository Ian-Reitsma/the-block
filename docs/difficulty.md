# Dynamic Difficulty Retargeting

The network retargets proof-of-work difficulty using a weighted moving average
of recent block intervals. Let `ts[i]` denote block timestamps in
milliseconds. The adjustment factor is the clamped ratio of the expected
interval to the observed average:

```
next = prev * clamp(avg_interval / 1000, 0.25, 4.0)
```

where `avg_interval` is the mean of `ts[i] - ts[i-1]` over the window. This
keeps difficulty within a 4× band while smoothly responding to hash‑rate
changes.

### Constants

| Name | Value | Description |
| --- | --- | --- |
| `DIFFICULTY_WINDOW` | `120` blocks | number of recent blocks considered |
| `DIFFICULTY_CLAMP_FACTOR` | `4` | max upward or downward adjustment |
| `TARGET_SPACING_MS` | `1_000` ms | target block interval |

These constants live in `node/src/consensus/constants.rs` and are consumed by
`expected_difficulty`.

### Worked Example

If the previous difficulty is `1000` and the last 120 blocks averaged
`4000 ms` apart, the ratio `avg_interval / TARGET_SPACING_MS` is `4`, so the
next difficulty becomes `prev * 4 = 4000`.

If blocks averaged `100 ms`, the ratio is `0.1`, which would drop below the
`1/DIFFICULTY_CLAMP_FACTOR` floor (`0.25`). Clamping yields `prev * 0.25 = 250`.

### Tests

Property-based tests in
[`node/tests/difficulty_retarget.rs`](../node/tests/difficulty_retarget.rs)
generate random and non-monotonic timestamp sequences to ensure the windowing
and clamping logic hold under edge cases.

Telemetry counters `difficulty_retarget_total` and `difficulty_clamp_total` report retarget executions and clamp events. Clients can query the current target via the `consensus.difficulty` RPC method.
