# Mempool QoS and Spam Controls

The mempool enforces a rolling fee floor computed from the 75th percentile of recent transaction fees. Transactions below this dynamic threshold trigger a wallet warning and may be rejected.

Each sender is limited to a fixed number of outstanding slots. When the mempool overflows, lowest-fee transactions are evicted first and counted via `mempool_evictions_total`.

`mempool/scoring.rs` contains the reputation-weighted scoring model. The current fee floor is exported as the `fee_floor_current` gauge for telemetry and explorer visualisation.
