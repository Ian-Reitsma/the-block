# Optional KYC Hooks
> **Review (2025-09-25):** Synced Optional KYC Hooks guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The node exposes an optional Know‑Your‑Customer (KYC) verification flow for
businesses that must vet participants. Verification is entirely off‑chain and
pluggable so the default build incurs no dependency or requirement. The
`node/src/kyc.rs` module ships with a `NoopKyc` provider that unconditionally
approves every request, plus a `KycProvider` trait for real adapters. Providers
may layer in caching, telemetry, and rate limiting without touching the rest of
the codebase.

| Provider       | Behaviour                     | Typical Use |
|----------------|-------------------------------|-------------|
| `NoopKyc`      | Always returns `true`          | Development / open networks |
| Custom adapter | Calls external API, honours cache TTL, records metrics | Compliance‑bound deployments |

## Plugin Interface

```rust
use the_block::kyc::{set_provider, KycProvider};

struct MyProvider;
impl KycProvider for MyProvider {
    fn verify(&self, user: &str) -> Result<bool, String> {
        // call out to third-party service
        Ok(user == "trusted")
    }
}

set_provider(Arc::new(MyProvider));
```

`verify` receives an opaque `user_id` string and should return `Ok(true)` when
the account passes checks or `Ok(false)` when the account is rejected. Network
errors bubble up via `Err(String)` and are surfaced to RPC callers.

## RPC Trigger

Clients may request verification via:

```json
{"method": "kyc.verify", "params": {"user": "alice"}}
```

The default provider always returns `"verified"`, preserving the current
bypass semantics when no plugin is installed.

### Request and Response

`kyc.verify` expects a JSON object with a `user` field:

```json
{"method": "kyc.verify", "params": {"user": "alice"}}
```

Successful checks yield `{ "status": "verified" }`. A negative decision
returns `{ "status": "denied" }`. Transport or provider failures raise an RPC
error with code `-32080`.

### Caching & Retries

Real providers should wrap API calls with a TTL cache so repeated lookups for
the same user do not saturate the upstream service. A typical implementation
stores `(user_id, verdict, expiry)` triples and invalidates entries when the TTL
elapses or when the provider signals revocation. Retry transient HTTP failures
with exponential backoff and jitter; abort permanently on 4xx errors.

### Telemetry

When the `telemetry` feature is enabled, providers are encouraged to increment
`kyc_success_total` and `kyc_failure_total` counters. Alerts should trigger if
failures exceed 1 % of requests or if the success rate drops below expected
thresholds.

### CLI Usage

Operators can wire a provider by exporting an API key and invoking the `kyc`
subcommand:

```bash
$ export KYC_API_KEY="sk_test_abc123"
$ cargo run -p cli -- kyc verify alice
alice: verified
```

`kyc set-key <key>` persists credentials for later reuse. The CLI forwards
requests to the node’s RPC endpoint, so it can be used for smoke tests during
deployment.

## On-Chain Impact

KYC results are not recorded on-chain and have no effect on consensus or
transaction processing. They merely gate application-level functionality when a
deployment opts in.
