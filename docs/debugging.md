# Debugger and Profiler
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

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