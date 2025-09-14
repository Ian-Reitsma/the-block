# Dynamic Difficulty Retargeting

The network retargets proof‑of‑work difficulty with a multi‑window exponential
moving average combined via Kalman‑style weights. Three windows (`short`,
`med`, `long`) track recent block intervals. Governance parameters
`kalman_r_short`, `kalman_r_med`, and `kalman_r_long` weight each window when
predicting the next interval. A `retune_hint` carried in each block header
applies a ±5 % pre‑adjustment for the following block.

For block timestamps `ts[i]` in milliseconds, let `ema_w` denote the EMA of
window `w`. The predicted interval is the weighted sum of these EMAs divided by
their total weight. The next difficulty scales the previous value by the ratio
`predicted / TARGET_SPACING_MS` and clamps to the range
`prev / DIFFICULTY_CLAMP_FACTOR .. prev * DIFFICULTY_CLAMP_FACTOR`.

### Constants

| Name | Value | Description |
| --- | --- | --- |
| `DIFFICULTY_WINDOW` | `120` blocks | timestamps retained for EMA calculations |
| `DIFFICULTY_CLAMP_FACTOR` | `4` | max upward or downward adjustment |
| `TARGET_SPACING_MS` | `1_000` ms | target block interval |

### Retune Hint

Each block stores a signed `retune_hint` summarizing the trend between short and
long windows. Positive values signal rising hash‑rate (difficulty should
increase), negative values indicate falling hash‑rate.

### CLI Inspection

Use the CLI to inspect recent retarget calculations:

```bash
blockctl difficulty inspect --last 5
```

The command prints the short/med/long EMAs, applied Kalman weights, and the
resulting difficulty hint for the latest blocks.

### Telemetry

Prometheus counters `difficulty_window_short`, `difficulty_window_med`, and
`difficulty_window_long` expose the raw EMA windows for dashboards.

### Tests

`tests/difficulty_retune.rs` simulates abrupt hash‑rate swings and verifies that
separate nodes converge on identical retunes.
