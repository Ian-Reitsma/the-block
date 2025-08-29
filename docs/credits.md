# Service Credits Ledger

Service credits track non-transferable balances used to pay for compute and network workloads. Each provider maintains a ledger entry persisted on disk so balances survive restarts.

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

