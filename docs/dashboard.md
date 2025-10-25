# Dashboard
> **Review (2025-09-25):** Synced Dashboard guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The node exposes a lightweight dashboard at `/dashboard` on the RPC HTTP port. The
page renders a small SPA that displays mempool depth, price bands, subsidy
counters, read-denial statistics (`read_denied_total{reason}`), LocalNet
statistics, and the latest ad-readiness snapshot (ready flag, unique viewers,
host/provider counts, configured minimums, and skip reasons). Operators can
point a browser at `http://<node>:<rpc_port>/dashboard` to view the metrics.

The dashboard is served as a static bundle from the node binary, requiring no additional assets at runtime.
