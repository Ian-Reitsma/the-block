# Dynamic Difficulty Retargeting

The network retargets proof-of-work difficulty using a weighted moving average
of the last 120 block intervals. Let `ts[i]` denote block timestamps in
milliseconds. The expected interval is 1000 ms. The adjustment factor is the
clamped ratio of the expected interval to the observed average:

```
next = prev * clamp(avg_interval / 1000, 0.25, 4.0)
```

where `avg_interval` is the mean of `ts[i] - ts[i-1]` over the window. This
keeps difficulty within a 4× band while smoothly responding to hash‑rate
changes.
