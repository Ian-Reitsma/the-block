# Dashboard

The node exposes a lightweight dashboard at `/dashboard` on the RPC HTTP port. The page renders a small SPA that will display mempool depth, price bands, credit balances, and LocalNet statistics. Operators can point a browser at `http://<node>:<rpc_port>/dashboard` to view the metrics.

The dashboard is served as a static bundle from the node binary, requiring no additional assets at runtime.
