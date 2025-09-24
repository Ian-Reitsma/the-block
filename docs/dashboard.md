# Dashboard
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The node exposes a lightweight dashboard at `/dashboard` on the RPC HTTP port. The page renders a small SPA that displays mempool depth, price bands, subsidy counters, read-denial statistics (`read_denied_total{reason}`), and LocalNet statistics. Operators can point a browser at `http://<node>:<rpc_port>/dashboard` to view the metrics.

The dashboard is served as a static bundle from the node binary, requiring no additional assets at runtime.