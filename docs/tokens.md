# Token Registry and Emission Schedules
> **Review (2025-09-25):** Synced Token Registry and Emission Schedules guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The chain supports multiple native tokens via a on-chain registry. Each token
specifies an emission schedule determining total supply over time.

## Registering Tokens

Governance proposals may register new tokens by specifying a symbol and
emission schedule. Once approved, nodes add the token to the registry and track
its supply.

## Emission Examples

- `Fixed(1_000_000)` – one million units minted at genesis.
- `Linear { initial: 0, rate: 10 }` – ten units minted every block.

## Metrics

The node exposes `tokens_created_total` and `token_bridge_volume_total`
runtime telemetry counters to monitor token issuance and bridge activity.
