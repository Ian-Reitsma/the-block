# Development Notes

## Async runtime facade

The node, CLI, and supporting services now target the shared `runtime` crate
instead of depending on Tokio primitives directly.  The facade exposes
`spawn`, `spawn_blocking`, `sleep`, `interval`, `timeout`, `yield_now`, and the
`select!` macro so production code can remain agnostic to the executor that is
selected at startup.

When migrating existing code:

- Replace `tokio::spawn`/`tokio::task::spawn` with `runtime::spawn`.  The helper
  keeps the metrics instrumentation that measures spawn latency and tracks
  pending tasks across both backends.
- Swap blocking helpers such as `tokio::task::spawn_blocking` and
  `tokio::runtime::Runtime::block_on` with the corresponding runtime facade
  calls.
- Use `runtime::sleep`, `runtime::interval`, `runtime::timeout`, and
  `runtime::yield_now` for timing primitives instead of `tokio::time::*`.
- Convert `tokio::select!` invocations to `runtime::select!`.  The macro routes
  to Tokio when the Tokio backend is active and falls back to the stub backend
  in unit tests or deterministic harnesses.
- Synchronous retry loops can call `runtime::block_on(runtime::sleep(..))` to
  reuse shared backoff logic while staying executor agnostic.

The facade honours the `TB_RUNTIME_BACKEND` environment variable so operators
and tests can swap in alternative implementations (for example, the
thread-assisted stub backend) without rewriting call sites.

## Guardrails for Tokio imports

Direct `tokio::` usage is now linted inside the `node` crate via
`#![deny(clippy::disallowed_methods)]` and `#![deny(clippy::disallowed_types)]`,
ensuring new code routes through the runtime facade.  The `tools/refcheck`
utility also scans workspace sources for forbidden Tokio symbols (e.g.
`tokio::spawn`, `tokio::time::sleep`, or `tokio::select!`) and runs in CI after
the dependency policy gate.  Any violations cause the job to fail, surfacing the
offending file and line so the call can be rewritten against the shared
abstraction.
