# Wallets
> **Review (2025-10-01):** Documented the first-party Ed25519 backend across CLI and wallet flows.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The `crates/wallet` crate implements deterministic Ed25519 key management, optional remote signer plumbing, and utilities for building transactions. The command-line interface lives in [`cli/src/wallet.rs`](../cli/src/wallet.rs) and is exposed through the `contract wallet` subcommand.

> **Build status (2025-10-01):** `node/src/bin/wallet.rs` now builds solely
> against the crypto suite’s in-house Ed25519 backend, forwards multisig signer sets, and passes the
> escrow hash algorithm into `verify_proof`. Focus on polishing multisig UX
> (batched signer discovery, richer operator messaging) before tagging the next
> CLI release.

## CLI overview

```
contract wallet help
contract wallet balances
contract wallet send --to <addr> --amount 25 --fee 5 --lane consumer --rpc http://127.0.0.1:26658
contract wallet send --to <addr> --amount 25 --fee 5 --auto-bump --json
contract wallet session --ttl 600
contract wallet meta-send --to <addr> --amount 10 --session-sk <hex>
contract wallet gen --out keystore.json   # requires the CLI to be built with --features quantum
```

`contract wallet balances` currently returns placeholder values; integrate it
with the node RPC once balance queries are exposed.

`contract wallet send` fetches the latest fee floor via `mempool.stats`, caches the response for ten seconds, and localises any warnings into English, Spanish, French, German, Portuguese, or Simplified Chinese based on `--lang`, `TB_LANG`, or the ambient `LANG`. When the chosen fee falls below the governance-configured floor the CLI offers to auto-bump to the minimum, force the submission as-is, or cancel. Non-interactive callers can set `--auto-bump` or `--force`, and `--json` returns a machine-readable envelope containing the payload, fee floor, and decision.

Example JSON output:

```json
{
  "status": "ready",
  "user_fee": 2,
  "effective_fee": 10,
  "fee_floor": 10,
  "lane": "consumer",
  "warnings": ["Warning: fee 2 is below the consumer fee floor (10)."],
  "auto_bumped": true,
  "forced": false,
  "payload": {"from_": "…", "to": "…", "fee": 10, "nonce": 0, "pct_ct": 100, "amount_consumer": 100, "amount_industrial": 0, "memo": []}
}
```

Every decision is reported back to the node through the `mempool.qos_event` RPC so telemetry captures `fee_floor_warning_total{lane}` and `fee_floor_override_total{lane}`. The CLI also exposes an `--ephemeral` flag that derives a one-off sender using `generate_keypair()` for quick testing.

## Session keys and meta-transactions

`contract wallet session --ttl <seconds>` emits a session key that can authorise delegated transactions for the given lifetime. The resulting key is compatible with `contract wallet meta-send`, which signs a payload on behalf of the root key holder. Session key helpers live in [`crypto/src/session.rs`](../crypto/src/session.rs) and surface lifecycle metrics (`session_key_issued_total`, `session_key_expired_total`).

## Key generation

`contract wallet gen --out <file>` writes Ed25519 and Dilithium key material when the CLI is built with the optional `quantum` feature (enabled via `cargo run -p contract-cli --features quantum -- wallet gen`). Without the feature the command prints a descriptive message and exits.

## Remote signer support

The wallet crate ships a reusable remote signer client in [`crates/wallet/src/remote_signer.rs`](../crates/wallet/src/remote_signer.rs). It discovers signers over UDP, supports HTTPS and mutually authenticated WebSockets, and exposes metrics such as `remote_signer_request_total`, `remote_signer_success_total`, and `remote_signer_error_total{reason}`. These counters are only exported when the node or Python bindings are built with the `telemetry` feature flag enabled; feature-off builds skip instrumentation but retain identical signing behaviour. Integrations can combine the remote signer with the CLI by building transactions via `wallet::build_tx` and forwarding the digest to the signer. Staking flows now return every approving key alongside its signature so the CLI can populate the RPC payload with

```json
{
  "id": "<primary signer>",
  "role": "gateway",
  "amount": 10,
  "sig": "<legacy single-sig fallback>",
  "threshold": 2,
  "signers": [
    {"pk": "<signer a>", "sig": "<signature>"},
    {"pk": "<signer b>", "sig": "<signature>"}
  ]
}
```

This keeps older nodes compatible (they continue to read `sig`) while giving upgraded deployments the data needed to enforce multisignature thresholds.

## Ed25519 dependency alignment

Both the wallet crate and the node CLI depend on the shared crypto suite’s
first-party Ed25519 backend (`crypto_suite::signatures::ed25519_inhouse`). The
upgrade replaces the legacy `ed25519` re-export with the modern `Signature`
API, ensuring remote signer payloads, staking flows, and explorer attestations
all share identical types without relying on `ed25519-dalek`. When migrating
custom tooling, update any `Signature::from_bytes` usage to
`Signature::from_slice` (which now returns a `Result`) and drop imports from the
deprecated `ed25519` crate. The wallet CLI also forwards the escrow `HashAlgo`
when invoking `verify_proof`, so third-party clients should include the same
field when constructing release proofs.

## Telemetry & monitoring

Wallet fee guidance ties into the node's telemetry pipeline. Every governance parameter activation (including fee-floor window and percentile changes) is recorded in `governance/history/fee_floor_policy.json` and mirrored by the explorer endpoint `/mempool/fee_floor_policy`. Operators should watch `fee_floor_window_changed_total`, `fee_floor_warning_total{lane}`, `fee_floor_override_total{lane}`, and `fee_floor_current` alongside remote signer counters when reviewing Grafana dashboards (`docs/mempool_qos.md`, `docs/monitoring/README.md`).

## Configuration tips

- JSON payloads from `contract wallet send --json` can be piped directly into automation that later invokes `contract wallet meta-send` or RPC submission helpers.
- Localisation defaults to English; override with `--lang es` or `TB_LANG=zh_CN` when scripting non-English experiences.
- The fee-floor cache is shared across lanes and keyed by the RPC URL. It currently retains responses for ten seconds (`FEE_FLOOR_CACHE_TTL` in [`cli/src/wallet.rs`](../cli/src/wallet.rs)); adjust the constant locally if shorter or longer horizons are required for experiments.
