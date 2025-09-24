# Governance Rollback
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Governance parameter changes may be reverted within the rollback window.

## Flow
1. Submit and pass a proposal adjusting a parameter.
2. After activation, call `gov rollback-last` before the window expires.
3. Metrics `param_change_active{key}` and `param_change_pending{key}` reset to their
   previous values.

## CLI

```
# check current status
cargo run --manifest-path examples/governance/Cargo.toml --bin gov status <id>

# rollback the most recent activation
cargo run --manifest-path examples/governance/Cargo.toml --bin gov rollback-last
```

See `examples/governance/` for proposal JSON examples.