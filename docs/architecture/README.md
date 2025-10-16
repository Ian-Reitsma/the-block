# Architecture
> **Review (2025-09-25):** Synced Architecture guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`node.md` contains the dependency tree for the `the_block` crate and `node-deps.svg` renders the first-level dependency graph. Both are generated with [`cargo tree`](https://crates.io/crates/cargo-tree) and [`cargo-deps`](https://crates.io/crates/cargo-deps).

Run `scripts/gen-architecture.sh` to refresh these artifacts. The `Architecture Docs Guard`
GitHub workflow enforces that `docs/architecture/node.md` and `docs/architecture/node-deps.svg`
are checked in after regeneration; CI will fail if the script produces changes. Install
Graphviz locally (`brew install graphviz` on macOS, `sudo apt-get install graphviz` on
Debian/Ubuntu) before running the script so the SVG output is generated alongside the Markdown.
