# Concurrency Primitives
> **Review (2025-09-25):** Synced Concurrency Primitives guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The codebase mixes `parking_lot` locks with the standard library's `std::sync`
mutexes and RwLocks. `parking_lot` locks never return a `Result` when acquiring
— the guard is returned directly and poisoning is not modelled — while
`std::sync::Mutex` and `RwLock` preserve poisoning semantics.

When editing modules make sure to match the existing primitive:

- Gossip and compute‑market hot paths (`node/src/net/peer.rs`,
  `node/src/compute_market/{matcher,scheduler,settlement}.rs`) use
  `parking_lot::Mutex` so guard access should never call `.unwrap()` or
  `.unwrap_or_else(...)`. Call `.lock()` and work with the returned guard
  directly.
- Global registries that rely on poisoning behaviour (e.g. ban store,
  governance state, RPC blockchain handles) intentionally use
  `std::sync::Mutex`. Those call sites still need `lock().unwrap_or_else(...)`
  guards to surface poisoning diagnostics.

Always double‑check the `use` statements in a module before wrapping a lock in
error handling, and prefer helper functions (such as
`peer_metrics_guard()` in `net::peer`) when a shared structure is accessed in
multiple places.
