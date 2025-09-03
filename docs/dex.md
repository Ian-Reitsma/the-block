# DEX and Trust Lines

The in-tree DEX exposes a simple order book with slippage checks and trust-line settlement.

- **Trust lines** carry a `limit`, `balance`, and `authorized` flag.
  Counterparties must explicitly authorize lines before any balance updates.
- **Order book** matches limit orders and rejects placements when the best
  available price exceeds the caller's `max_slippage_bps`.
- **Settlement** adjusts trust-line balances between buyer and seller for each
  trade. Path finding over authorized lines allows multi-hop payments.
- **Routing** uses cost-based path scoring and returns a fallback route when the
  cheapest path later fails. The primary path minimizes hop count and a secondary
  path is returned if one exists.
- **Persistence** stores books and executed trades under `~/.the_block/state/dex/`
  via a bincode-backed `DexStore`. Order books are rebuilt on startup so restarts
  or crashes do not lose market depth, and trade logs allow explorers to replay
  historical fills.
- **Metrics** expose `dex_orders_total{side=*}` and `dex_trades_total` counters
  along with per-hop routing costs for observability.

The pool invariant used for swaps is the del‑Pino logarithmic curve

\[
x \ln x + y \ln y = k
\]

which ensures arbitrage‑free paths even under clustered volatility. A
governance parameter $\varepsilon$ adds virtual reserves to bound
slippage in thin pools: the solver operates on $(x+\varepsilon, y+\varepsilon)$.
See `sim/src/dex.rs` for the reference implementation.

Example:

```rust
use the_block::dex::{OrderBook, Order, Side, TrustLedger};
# fn main() {
let mut book = OrderBook::default();
let mut ledger = TrustLedger::default();
ledger.establish("alice".into(), "bob".into(), 100);
ledger.authorize("alice", "bob");
let buy = Order { id:0, account:"alice".into(), side:Side::Buy, amount:10, price:5, max_slippage_bps:0 };
let sell = Order { id:0, account:"bob".into(), side:Side::Sell, amount:10, price:5, max_slippage_bps:0 };
book.place(buy).unwrap();
book.place_and_settle(sell, &mut ledger).unwrap();
assert_eq!(ledger.balance("alice", "bob"), 50);
# }
```
