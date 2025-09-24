# Architecture
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

`node.md` contains the dependency tree for the `the_block` crate and `node-deps.svg` renders the first-level dependency graph. Both are generated with [`cargo tree`](https://crates.io/crates/cargo-tree) and [`cargo-deps`](https://crates.io/crates/cargo-deps).

Run `scripts/gen-architecture.sh` to refresh these artifacts.