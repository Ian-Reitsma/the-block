# Debugger and Profiler
> **Review (2025-09-25):** Synced Debugger and Profiler guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

A lightweight debugger `tb-debugger` allows inspection of the node's
embedded `SimpleDb`. Example usage:

```bash
cargo run -p tb-debugger -- --db node-data get some-key
```

List keys with a prefix using `keys <prefix>`.

The node binary supports optional CPU profiling. Launch with
`TB_PROFILE=1` to emit `flamegraph.svg` upon shutdown:

```bash
TB_PROFILE=1 cargo run -p the_block -- run
```

Flamegraphs help identify hotspots for optimisation. The profiler uses
sampling and incurs minimal overhead when disabled.
