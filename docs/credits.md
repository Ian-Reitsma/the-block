# Service Credits Ledger

Service credits track non-transferable balances used to pay for compute and network workloads. Each provider maintains a ledger entry persisted on disk so balances survive restarts.

Reads are free; credits are only burned on write operations such as storing new objects.

## Ledger Operations

The `credits` crate exposes APIs for:

- `accrue(provider, amount, tag)` – award credits for completed jobs or service badges.
- `spend(provider, amount)` – deduct credits when consuming services.
- `balance(provider)` – query the current balance.

Balances are stored in a `sled` tree keyed by provider ID. Updates use `compare_and_swap` to avoid lost writes and duplicate events are ignored by tagging entries with a unique event ID.

### CLI Usage

The node binary exposes a `credits` subcommand for inspection and manual adjustments:

```bash
cargo run --bin node -- credits top-up --provider alice --amount 100
cargo run --bin node -- credits balance alice
cargo run --bin node -- credits transfer --from alice --to bob --amount 5
```

All commands persist changes through the ledger crate so subsequent runs observe the updated totals. Temporary directories in tests use isolated sled paths to avoid cross-test contamination.

### Settlement Integration

Compute-market receipts settle against the ledger in `Real` mode, debiting buyers and crediting providers. See [`compute_market.md`](compute_market.md#receipt-settlement-and-credits-ledger) for details on receipt formats and idempotent application.

### Examples

Sample governance workflows demonstrating credit usage live under [`examples/governance/`](../examples/governance/). The `CREDITS.md` file lists common commands, and the `gov status` output exposes rollback metrics alongside current balances.

## Issuance Sources

Credits accrue from distinct sources so rewards can be tuned independently:

- `Uptime` – awarded for keeping nodes online.
- `LocalNetAssist` – granted for validated local networking help.
- `ProvenStorage` – credited for storage proofs.
- `Civic` – community chores and governance duties.

Weights and per-identity or per-region caps are controlled by `credits.issuance.*`
governance parameters. Prometheus counters
`credit_issued_total{source}` and `credit_issue_rejected_total{reason}` expose
issuance behaviour.

## Decay and Expiry

Balances decay exponentially so idle credits eventually lapse. Each provider
record stores a `last_update` timestamp; `decay_and_expire(now)` scales balances by
`exp(-λ·Δt)` and zeroes sources that exceed their configured expiry window. The
decay rate `credits.decay.lambda_per_hour_ppm` and per-source
`credits.expiry_days.*` windows are adjustable via governance. Settlement invokes
the decay routine on every hourly tick before applying new receipts.


