# Mobile Gateway

A lightweight RPC gateway serves mobile clients with a local cache to reduce bandwidth and latency.

## Caching
Responses are cached with a TTL and served from memory when fresh. Hits increment `mobile_cache_hit_total`.

## Offline Queue
Transactions submitted while offline are queued and replayed on reconnect. The current queue depth is exported as `mobile_tx_queue_depth`.

Operators can inspect runtime stats with `gateway mobile-stats`.
