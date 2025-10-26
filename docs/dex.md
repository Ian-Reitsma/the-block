# DEX and Trust Lines
> **Review (2025-10-13):** Documented the cursor-based persistence stack for order books, trade logs, AMM pools, and escrow snapshots (`node/src/dex/{storage.rs,storage_binary.rs}`) plus the new `EscrowSnapshot` helper that replaces the legacy serde/binary-codec shim. Regression tests (`order_book_matches_legacy`, `trade_log_matches_legacy`, `escrow_state_matches_legacy`, `pool_matches_legacy`) and randomized property coverage keep sled bytes aligned with historical layouts while the DEX crate’s escrow tests validate snapshot round-trips.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, codec, and serialization wrappers are live with governance overrides enforced (2025-10-13).

The in-tree DEX exposes a simple order book with slippage checks and trust-line settlement. Routing logic supports multi-hop transfers over a graph of authorised trust lines and returns a fallback path when the cheapest route fails mid-flight.

## 1. Trust Lines

Trust lines track bilateral credit with three fields:

- `limit` – maximum absolute IOU value permitted between parties.
- `balance` – signed current exposure. Positive means the first party owes the second.
- `authorized` – both sides must call `authorize(a, b)` before any balance adjustments occur.

`TrustLedger` stores all lines in a hash map keyed by `(a, b)`. Establishing a line inserts a zero balance with the chosen limit.

## 2. Order Book and Settlement

`node/src/dex/order_book.rs` maintains price-sorted `buy` and `sell` heaps. Orders specify `amount`, `price`, and `max_slippage_bps`.

- When matching, the engine checks that the counter-order price does not exceed the caller's slippage tolerance.
- Each fill calls `TrustLedger::adjust` to move balances along the settlement path.
- Trades, pools, and escrow state persist to `~/.the_block/state/dex/` via a
  `DexStore` that now routes through the first-party cursor helpers in
  `node/src/dex/{storage.rs,storage_binary.rs}`, removing the legacy
  `binary_codec` shim while keeping sled keys and byte layout compatible with
  historical deployments.

### Persistence format

- `node/src/dex/storage_binary.rs` encodes order books, trade logs, AMM pools,
  and escrow snapshots with the shared binary cursor helpers and
  `binary_struct` assignment utilities. Each structure has a regression test
  that round-trips legacy sled bytes (`order_book_matches_legacy`,
  `trade_log_matches_legacy`, `escrow_state_matches_legacy`,
  `pool_matches_legacy`) plus randomized coverage to hammer order depth,
  partial-proof trees, and liquidity weights.
- `EscrowState` no longer derives serde; the snapshot/export helpers rely on
  the new `EscrowSnapshot` container (`dex/src/escrow.rs`) so storage callers
  can persist and restore the BTree map, next identifier, and aggregate locked
  total without touching third-party codecs.
- The DEX crate ships a `snapshot_roundtrip` unit test that exercises the
  snapshot helpers directly. Add CLI/explorer integration coverage when
  migrating user-facing tooling to the new codec.

## 2.1 Escrow and Partial Payments

Before settlement, matched orders lock funds in an on-ledger escrow. The escrow entry tracks `from`, `to`, the locked total, and a
Merkle root over released partial payments. Each release appends a payment amount and recomputes the root, yielding a proof that
can be verified off-chain. When the cumulative released amount equals the original total the escrow entry is removed and the
trade is final.

`Escrow::snapshot()` exports the in-memory table into an `EscrowSnapshot`
capturing the ordered entries, the next identifier, and the aggregate locked
total. `Escrow::from_snapshot()` restores that state so `DexStore` can persist
and reload escrows purely through the first-party codecs.

## 2.2 Deterministic Liquidity Router

`node/src/liquidity/router.rs` unifies DEX escrows, bridge withdrawals, and
trust-line rebalancing under a shared, deterministic scheduler. The router
collects three intent types on each planning tick:

- **`DexEscrow`** – in-flight escrows (`EscrowState::locks`) are sorted by their
  `locked_at` timestamp and batched so the oldest trades settle first. Before
  releasing funds, the router re-runs `find_best_path` to guarantee a
  trust-line path exists and then executes the transfer via
  `TrustLedger::settle_path`.
- **`BridgeWithdrawal`** – withdrawals past their challenge deadline are pulled
  from `Bridge::pending_withdrawals` and finalised through
  `Bridge::finalize_withdrawal`, keeping bridge commitments and escrow releases
  sequenced together.
- **`TrustRebalance`** – positive trust-line imbalances above a configured
  threshold are routed through `find_best_path`/fallback pairs and settled to
  keep multi-hop credit loops balanced.

To resist front-running, each batch derives a deterministic tie-breaker by
hashing the previous block entropy and the intent fingerprint. A fairness
window injects jitter within the intent’s priority band so operators can audit
ordering without leaking the final sequence ahead of time.

Governance surfaces four knobs via `RouterConfig`:

| Parameter | Description |
| --- | --- |
| `batch_size` | Maximum intents executed per tick. Larger values drain queues faster at the cost of longer single-batch commits. |
| `fairness_window` | Maximum jitter (default 250 ms) applied when ranking intents, providing MEV resistance while keeping auditability. |
| `max_trust_hops` | Caps how many trust-line edges a rebalance may traverse; routes exceeding the cap fall back to secondary paths. |
| `min_trust_rebalance` | Minimum outstanding balance (in destination units) before a trust-line rebalance intent is emitted. |

The router returns a `LiquidityBatch` describing the ordered intents plus their
assigned slots. Execution helpers in the DEX and bridge modules apply each
intent atomically so escrow releases, bridge finalisations, and credit
rebalancing share the same ledger commit window.

### CLI Escrow Lifecycle

```bash
# check pending escrow 7 including outstanding amount and proofs
blockctl dex escrow status 7
# {
#   "from": "alice",
#   "to": "bob",
#   "total": 100,
#   "released": 20,
#   "outstanding": 80,
#   "proofs": [{"amount":20,"proof":["aa..","bb.."]}]
# }

# release 40 units from escrow 7 and verify proof
blockctl wallet escrow-release 7 40
# released with proof
```

`blockctl` wraps the `dex.escrow_status` and `dex.escrow_release` RPC calls. Each
release updates the Merkle root stored with the escrow so both sides can audit
partial payments.

### Proof Verification via RPC

Obtain a Merkle proof for a prior release and verify it off-chain:

```bash
curl -s localhost:26658/dex.escrow_proof?id=7\&index=1 | jq
# {"amount":40,"proof":["aa..","bb.."]}
```

Clients recompute the root from the provided `amount` and `proof` to confirm the
release was recorded.

### Telemetry

`dex_escrow_locked` and `dex_liquidity_locked_total` gauge total funds locked in
escrow, while `dex_escrow_pending` counts outstanding escrows. Operators can
alert on any metric to detect stuck settlements. See
[`docs/telemetry.md`](telemetry.md) for the full metric list.

### Failure Modes and Recovery

- **Timeout releases:** if one side disappears, the remaining balance can be cancelled and returned after a timeout.
- **Invalid proofs:** releases providing hashes that do not match the stored root are rejected and the escrow remains pending for
  manual recovery.

## 3. Multi-Hop Routing Algorithms

`node/src/dex/trust_lines.rs` implements two path finders:

### 3.1 Breadth-First Search (`find_path`)

Used for quick reachability checks when no costs are attached. It performs a BFS over authorised edges requiring each hop to have at least `amount` headroom (`limit >= |balance| + amount`). Returns the first path discovered.

### 3.2 Slack-Aware Search with Fallback (`find_best_path`)

For production routing the ledger now balances *capacity* against *hop count*:

1. `max_slack_path` walks the trust graph with a max-heap ordered by the
   smallest residual headroom on the path so far. The primary route is the one
   that maximises the minimum slack (`limit - (|balance| + amount)`) while still
   respecting authorisations and credit limits. This favours wider corridors
   that are less likely to saturate in follow-up batches, even if they require
   extra hops.
2. The traditional hop-count `dijkstra` still runs to recover the shortest path
   for fallback purposes.
3. If the slack-optimised path and the shortest path differ, the latter becomes
   the fallback. Otherwise the router searches for a disjoint alternative by
   excluding the primary edges and re-running `dijkstra`.

`find_best_path` returns `(primary, fallback)` where `fallback` is `None` if no
hop-limited or disjoint alternative exists. The liquidity router consumes the
pair, automatically downgrading to the fallback when governance-configured hop
limits would otherwise reject the primary route.

## 4. Monitoring

- Metrics `dex_orders_total{side=*}` and `dex_trade_volume` track order flow and matched quantity.
- Per-hop routing costs and fallback usage are exported when telemetry is enabled, helping operators spot imbalances or saturated lines.

## 5. Example

```rust
use the_block::dex::{storage::EscrowState, OrderBook, Order, Side, TrustLedger};

let mut book = OrderBook::default();
let mut ledger = TrustLedger::default();
ledger.establish("alice".into(), "bob".into(), 100);
ledger.establish("bob".into(), "carol".into(), 100);
ledger.authorize("alice", "bob");
ledger.authorize("bob", "carol");
let buy = Order { id:0, account:"alice".into(), side:Side::Buy, amount:10, price:5, max_slippage_bps:0 };
let sell = Order { id:0, account:"carol".into(), side:Side::Sell, amount:10, price:5, max_slippage_bps:0 };
book.place(buy).unwrap();
let mut esc_state = EscrowState::default();
book.place_and_lock(sell, &mut esc_state).unwrap();
let eid = *esc_state.locks.keys().next().unwrap();
esc_state.escrow.release(eid, 50).unwrap();
ledger.adjust("alice", "carol", 50);
ledger.adjust("carol", "alice", -50);
assert_eq!(ledger.balance("alice", "bob"), 50);
assert_eq!(ledger.balance("bob", "carol"), 50);
```

## 6. Further Reading

- Routing implementation: `node/src/dex/trust_lines.rs`.
- Economic rationale and governance for fees: `docs/fees.md`.
- Progress and remaining gaps for the DEX: `docs/progress.md` §7.

Keep this doc updated as routing metrics, cost functions, or persistence formats evolve.

### 5.1 Test Coverage

- `node/tests/liquidity_router.rs` now exercises multi-batch fairness (excess
  intents spill into deterministic follow-up batches), challenged bridge
  withdrawals (challenged commitments are skipped), and hop-limited fallback
  behaviour (the router selects the direct path when the slack-optimised route
  would exceed `max_trust_hops`).
- `node/tests/trust_routing.rs` verifies that `find_best_path` favours the
  highest-slack route while still exposing a shorter fallback for hop-limited
  schedulers.
