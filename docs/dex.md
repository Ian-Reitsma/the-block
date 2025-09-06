# DEX and Trust Lines

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
- Trades and order placements persist to `~/.the_block/state/dex/` via a bincode-backed `DexStore`, surviving crashes and restarts.

## 2.1 Escrow and Partial Payments

Before settlement, matched orders lock funds in an on-ledger escrow. The escrow entry tracks `from`, `to`, the locked total, and a
Merkle root over released partial payments. Each release appends a payment amount and recomputes the root, yielding a proof that
can be verified off-chain. When the cumulative released amount equals the original total the escrow entry is removed and the
trade is final.

### Failure Modes and Recovery

- **Timeout releases:** if one side disappears, the remaining balance can be cancelled and returned after a timeout.
- **Invalid proofs:** releases providing hashes that do not match the stored root are rejected and the escrow remains pending for
  manual recovery.

## 3. Multi-Hop Routing Algorithms

`node/src/dex/trust_lines.rs` implements two path finders:

### 3.1 Breadth-First Search (`find_path`)

Used for quick reachability checks when no costs are attached. It performs a BFS over authorised edges requiring each hop to have at least `amount` headroom (`limit >= |balance| + amount`). Returns the first path discovered.

### 3.2 Dijkstra with Fallback (`find_best_path`)

For optimal routing, `dijkstra` assigns each hop a cost of `1` (one trust-line traversal). The algorithm:

1. Initialises a min-heap ordered by cumulative hop count.
2. Pops the lowest-cost node, exploring authorised neighbours that still have credit for `amount`.
3. Records predecessor pointers to reconstruct the cheapest path to `dst`.
4. After the primary path is found, edges along that path are excluded and a second `dijkstra` run searches for a fallback route.

`find_best_path` returns `(primary, fallback)` where `fallback` is `None` if no disjoint route exists.

## 4. Monitoring

- Metrics `dex_orders_total{side=*}` and `dex_trades_total` track activity.
- Per-hop routing costs and fallback usage are exported when telemetry is enabled, helping operators spot imbalances or saturated lines.

## 5. Example

```rust
use the_block::dex::{OrderBook, Order, Side, TrustLedger};

let mut book = OrderBook::default();
let mut ledger = TrustLedger::default();
ledger.establish("alice".into(), "bob".into(), 100);
ledger.establish("bob".into(), "carol".into(), 100);
ledger.authorize("alice", "bob");
ledger.authorize("bob", "carol");
let buy = Order { id:0, account:"alice".into(), side:Side::Buy, amount:10, price:5, max_slippage_bps:0 };
let sell = Order { id:0, account:"carol".into(), side:Side::Sell, amount:10, price:5, max_slippage_bps:0 };
book.place(buy).unwrap();
book.place_and_settle(sell, &mut ledger).unwrap();
assert_eq!(ledger.balance("alice", "bob"), 50);
assert_eq!(ledger.balance("bob", "carol"), 50);
```

## 6. Further Reading

- Routing implementation: `node/src/dex/trust_lines.rs`.
- Economic rationale and governance for fees: `docs/fees.md`.
- Progress and remaining gaps for the DEX: `docs/progress.md` §7.

Keep this doc updated as routing metrics, cost functions, or persistence formats evolve.
