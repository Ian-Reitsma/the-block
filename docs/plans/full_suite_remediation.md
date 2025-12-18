# Full Suite Remediation Plan — 2024‑XX‑XX

Context: `cargo test --all --no-fail-fast` currently fails in four clusters (treasury sled store semantics, wallet remote-signer harnesses, CLI doctests, and economics/DNS doctests). This note records a verbose remediation plan so future-me remembers the whys/how.

## 1. Treasury Store vs. Test Expectations
- Failing test: `node/tests/treasury.rs::node_treasury_accrual_flow` asserts the final snapshot stays `(54, 12)` after calling `cancel_disbursement`.
- Actual behavior (per `governance/store.rs:2533-2665`) is to append a compensating ledger entry whenever an executed disbursement is rolled back. That matches the spec in `docs/economics_and_governance.md` (“Rollbacks simply … append a compensating ledger entry”).
- **Plan:**
  1. Update the test assertion to expect `(64, 16)` after canceling an executed disbursement, and add a short comment pointing back to the spec section so reviewers know the behavior is intentional.
  2. While touching the test, add an assertion that the `history` vector actually contains both the executed and cancelled markers so we cover the full state machine.
  3. Re-run `cargo test -p the_block --test treasury` to prove the sled store + history agree.

## 2. Wallet Remote-Signer (single + multisig) Harness Failures
- Symptoms:
  - `remote_signer_mtls_ws` fails to connect with `os error 35` (“Resource temporarily unavailable”).
  - Multisig tests print `[ACCEPT] would block` forever before timing out, poison the serial mutex, and cascade failures.
- Root causes:
  1. The mock servers in `crates/wallet/tests/support/mod.rs` bind listeners via `runtime::net::TcpListener`, which is flaky in the sandbox (same accept loop issue I fixed earlier for WebSocket tests).
  2. `RemoteSigner::sign_ws` treats `WouldBlock`/`EINPROGRESS` as fatal instead of retrying, so even transient failures abort the test.
- Most recent update: Even with the harness fixes, `remote_signer_mtls_ws` still failed because `httpd` deliberately disabled WebSocket upgrades on TLS listeners (`TlsStream::supports_websocket` returned `false`). The MTLS signer hangs inside the WebSocket handshake until the server rejects the upgrade with a 400, which the test then surfaces as `server declined websocket upgrade`. The remediation is to allow TLS-based upgrades by returning `true` from `supports_websocket` so the HTTP server can hand the decrypted stream to the WebSocket router. **Previously** the HTTP-only round-trips were the reliable cluster; now they are the regressions: `remote_signer_roundtrip`, `remote_signer_signature_roundtrip_bytes`, `remote_signer_threshold_error`, and `remote_signer_timeout` all panic with `connect: Failure("timeout")` while fetching `/pubkey`, and every multisig scenario times out after logging four `retrying signer request ... error=timeout` warnings (see `crates/wallet/tests/remote_signer.rs:13-90` and `crates/wallet/tests/remote_signer_multisig.rs:12-94`). The shared root cause is that the synchronous client blocks on a pubkey fetch while the mock server waits for the runtime to schedule `httpd::serve_stream`—but we never drive the runtime once the serial mutex is held. The blocking fetch eventually hits the 5s `ClientConfig::request_timeout`, bubbles up as `"timeout"`, poisons the serial mutex, and every subsequent test hits the `PoisonError`. Fixing this means running the entire signer mock inside an async task (`runtime::spawn`) and only using `spawn_blocking` for the outer `StdTcpListener`, or alternatively switching the tests themselves to `runtime::block_on(async { ... })` so the runtime can actually poll the connection handlers.
- **Plan:**
  1. Extend `runtime::net::TcpListener` and the in-house backend to expose a `from_std` constructor so tests can accept connections with a blocking `std::net::TcpListener` (which is reliable under the sandbox) and still reuse the first-party HTTP router.
  2. Add a public `httpd::serve_stream` API that wraps the current private `handle_connection` so tests can feed converted streams directly without going through the flaky async accept loop.
  3. Rewrite `spawn_httpd` (and the TLS variant) in `wallet/tests/support` to:
     - bind a blocking `std::net::TcpListener` on `127.0.0.1:0`,
     - spawn a dedicated thread that accepts sockets, converts each to `runtime::net::TcpStream::from_std`, and hands it to the new `httpd::serve_stream` helper,
     - keep the async router + TLS configs untouched.
  4. Harden `RemoteSigner::sign_ws` (and the HTTPS pubkey fetcher) so that `io::ErrorKind::WouldBlock` / raw errno 35 triggers a short backoff/retry instead of an immediate failure.
  5. Re-run `cargo test -p wallet --test remote_signer` and `cargo test -p wallet --test remote_signer_multisig` locally to make sure all scenarios (success, invalid sig, timeout, TLS) pass with the new harness.

## 3. CLI Doc-Tests (`contract-cli`)
- `rustdoc` cannot resolve the `explorer` dependency when it compiles doctests, which stops `cargo test --doc -p contract-cli` before any snippets run.
- Hypothesis: the doc build doesn’t pull in heavyweight dependencies, so the easiest path is to gate the explorer module during `cfg(doc)` instead of forcing the explorer sled stack to build.
- **Plan:**
  1. Wrap the `pub mod explorer;` export (and the corresponding `use explorer::…` statements) in `#[cfg(any(not(doc), feature = "full"))]`.
  2. Under `cfg(doc)` define a tiny stub (`struct ExplorerStore; impl ExplorerStore { fn open(_: &str) -> Self { … } }`) so doc comments/examples referencing `ExplorerStore` continue to compile.
  3. Leave runtime behavior unchanged because `cfg(doc)` is only enabled by rustdoc.
  4. Re-run `cargo test -p contract-cli --doc` to make sure the stub satisfies rustdoc without affecting the real binary.

## 4. Economics & DNS Doc Comments
- Doctest errors: all formulas that use the Unicode multiplication sign (`×`) are treated as actual Rust code, so rustdoc tries to compile them and chokes on the Unicode tokens/missing bindings.
- Affected locations:
  - `node/src/economics/network_issuance.rs` (`block_reward` and the helper formulas)
  - `node/src/gateway/dns.rs::compute_dynamic_reserve_price` (reserve price formula)
- **Plan:**
  1. Change every formula block to ` ```text ` (or ` ```ignore `) so rustdoc renders them but doesn’t execute them.
  2. Replace `×` with ASCII `*` inside those fenced blocks to avoid surprises in environments that don’t like non-ASCII tokens.
  3. Double-check `cargo fmt` keeps the doc comments readable.
  4. Re-run `cargo test -p the_block --doc` to ensure the doctest harness no longer tries to compile math prose.

## 5. HTTPd WebSocket Harness Regression
- Symptoms:
  - `crates/httpd/tests/server.rs::websocket_upgrade_accepts_and_dispatches_handler` now panics with `unexpected frame: Some(Close(Some(CloseFrame { code: 1006, reason: "abnormal closure" })))` after the handshake succeeds. That tells us the server side dropped the TCP stream before the handler could flush the `WsMessage::Text("hello")` payload.
  - `websocket_upgrade_over_tls_dispatches_handler` used to assert that TLS listeners reject upgrades. After flipping `TlsStream::supports_websocket()` to return `true` (see `crates/httpd/src/lib.rs:608-683`) and wiring TLS sockets through `runtime::ws::ServerStream`, the test still expects an HTTP 400 and panics once it sees the negotiated `HTTP/1.1 101` status.
- Root cause:
  1. We now detach the upgrade future via `runtime::spawn` (`crates/httpd/src/lib.rs:1503-1515`) but drop the resulting `JoinHandle`, so any IO error surfaced by the handler is only logged at `debug!` and the caller never sees it. When we enabled TLS upgrades we also tightened the server response path, so the handler now tries to write into a stream whose HTTP half has already been torn down, producing the synthesized 1006 close code. The client test catches that immediately.
  2. The TLS guardrail baked into `websocket_upgrade_over_tls_dispatches_handler` is officially obsolete because the runtime now supports WebSockets over `TlsStream`. The test (and doc comments) still describe the pre-change contract, so it asserts `HTTP/1.1 400` and fails as soon as the upgraded 101 headers arrive.
- **Plan:**
  1. Teach the upgrade branch in `handle_connection` to keep the `JoinHandle` alive and bubble `HttpError`s back to the caller. The simplest fix is to store the handle in the per-connection task list (next to `keep_alive`) and make sure the handler completes (or logs) before we drop the stream. While in there, add a targeted log at `crates/httpd/tests/server.rs:760-788` so regressions surface as actual IO errors rather than synthesized close codes.
  2. Update `websocket_upgrade_over_tls_dispatches_handler` to assert the new behavior: expect `HTTP/1.1 101`, read the “hello” frame via `runtime::ws::ClientStream`, and verify the handler closes the stream cleanly. Add a doc note in `docs/architecture.md#gateway-and-client-access` explaining that TLS upgrades are now first-class so operators stop relying on the plaintext-only statement from the old spec.
  3. Extend the test matrix so we cover both the plaintext and TLS paths. Right now both tests use bespoke harnesses; once the handler reliably emits frames we can factor the handshake helper into one place and cover failure modes (missing `Upgrade` header, bad key, etc.) in separate cases.

## 6. In-House Transport Loopback Break
- Symptoms: `crates/transport/tests/inhouse.rs::handshake_success_roundtrip` fails at `recv payload` even though `handshake_success_roundtrip` already completed `adapter.listen`, `adapter.connect`, and `adapter.send`. The failure bubbles up as `Option::expect` because `adapter.recv(&conn)` returned `None`.
- Root cause: `Connection::recv` simply forwards to the `mpsc::UnboundedReceiver` fed by `client_receiver_loop` (`crates/transport/src/inhouse/adapter.rs:520-618`). That loop exits whenever `recv_datagram` errors, and `recv_datagram` temporarily removes the `UdpSocket` from `self.inner.socket`. The test drops the `Endpoint` immediately after grabbing `local_addr` (`crates/transport/tests/inhouse.rs:31-47`), so the server loop shuts down as soon as the handshake completes. When we later call `adapter.send`, the client transmits the datagram but `server_loop` has already stopped acking, so the receiver loop observes `Err(ErrorKind::NotConnected)` and closes the channel. From that point on `adapter.recv` always returns `None`.
- **Plan:**
  1. Keep the listener alive until after we have verified the round-trip. In practice that means keeping the `Endpoint` guard around (even if it is `_listener_guard`) so the shutdown token in `EndpointInner` (`crates/transport/src/inhouse/adapter.rs:332-360`) isn’t tripped prematurely.
  2. Add an explicit regression test that drops the listener before sending data and asserts that we return a meaningful error (e.g., `TransportError::ListenerClosed`) instead of propagating a `None`.
  3. Once the lifetime bug is fixed, re-run `cargo test -p transport --test inhouse handshake_success_roundtrip` under both runtimes to confirm the ack path survives the longer-lived listener.

## Verification Checklist
- `cargo test -p the_block --test treasury`
- `cargo test -p wallet --test remote_signer`
- `cargo test -p wallet --test remote_signer_multisig`
- `cargo test -p contract-cli --doc`
- `cargo test -p the_block --doc`
- Final sweep: `cargo test --all --no-fail-fast`

Documenting this plan up-front satisfies the spec-first contract (see `AGENTS.md §0.1`). The concrete edits will reference this note in their commit messages / PR descriptions.
