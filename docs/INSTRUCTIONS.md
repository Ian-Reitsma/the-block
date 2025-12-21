# Market Receipt System: Developer Integration Guide

**Last Updated**: 2025-12-18
**Status**: Production-ready
**Complexity**: Moderate (requires understanding of block construction flow)

## Executive Summary

The Block uses a **deterministic receipt emission system** to create cryptographic commitments of market activity on-chain. Every market settlement (storage proofs, compute jobs, energy delivery, ad campaigns) emits a receipt that gets included in blocks, enabling:

1. **Deterministic economics replay** - Derive market metrics from blocks without live market state
2. **Auditability** - Cryptographic proof of all settlements
3. **Light client verification** - Verify market activity without running full markets
4. **Cross-chain bridges** - Portable proof of economic activity

This guide covers the **complete architecture**, not surface-level docs. You'll understand how receipts flow from market settlement → block inclusion → telemetry → economics derivation.

---

## Architecture Overview

### Receipt Flow (Complete Path)

```
Market Settlement
    ↓
Market emits receipt to pending_receipts: Vec<Receipt>
    ↓
Block construction calls drain_receipts()
    ↓
Receipts added to Block.receipts: Vec<Receipt>
    ↓
Block mined and gossiped
    ↓
Telemetry records receipt counts/sizes
    ↓
Economics derivation uses receipts for metrics
```

### Key Invariants

1. **Receipt emission is atomic with settlement** - If a market accrues payment, a receipt MUST be emitted
2. **Receipts are drained exactly once per block** - No double-counting, no loss
3. **Block height is set at emission time** - Receipts know which block they settle in
4. **Receipts are append-only** - Never modified after creation

---

## Implementation Guide: Adding a New Market

### Phase 1: Define the Receipt Structure

**File**: `node/src/receipts.rs`

```rust
/// Your market's receipt variant
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct YourMarketReceipt {
    /// Unique identifier for this settlement (contract ID, job ID, etc.)
    pub settlement_id: String,

    /// Provider/seller address
    pub provider: String,

    /// Units of work (bytes, compute units, kWh, impressions)
    pub units: u64,

    /// Payment in consumer tokens (CT)
    pub payment_ct: u64,

    /// Block height when settled
    pub block_height: u64,

    /// Verification proof (SNARK, meter reading, etc.)
    pub proof_data: [u8; 32],
}
```

**Add to enum**:

```rust
pub enum Receipt {
    Storage(StorageReceipt),
    Compute(ComputeReceipt),
    Energy(EnergyReceipt),
    Ad(AdReceipt),
    YourMarket(YourMarketReceipt), // ← ADD THIS
}
```

**Update helper methods**:

```rust
impl Receipt {
    pub fn market_name(&self) -> &'static str {
        match self {
            // ... existing cases
            Receipt::YourMarket(_) => "your_market",
        }
    }

    pub fn settlement_amount(&self) -> u64 {
        match self {
            // ... existing cases
            Receipt::YourMarket(r) => r.payment_ct,
        }
    }

    pub fn block_height(&self) -> u64 {
        match self {
            // ... existing cases
            Receipt::YourMarket(r) => r.block_height,
        }
    }
}
```

### Phase 2: Market-Side Receipt Emission

**Pattern A: Market has state struct (like Storage/Compute)**

```rust
// your_market/src/lib.rs or your_market/src/receipts.rs

pub struct YourMarketReceipt {
    // Match the canonical format but avoid circular deps
    pub settlement_id: String,
    pub provider: String,
    pub units: u64,
    pub payment_ct: u64,
    pub block_height: u64,
    pub proof_data: [u8; 32],
}

pub struct YourMarket {
    settlements: HashMap<String, Settlement>,
    pending_receipts: Vec<YourMarketReceipt>, // ← Receipt storage
}

impl YourMarket {
    pub fn settle(&mut self, settlement_id: &str, current_block: u64) -> Result<u64, Error> {
        let settlement = self.settlements.get(settlement_id)?;

        // Perform settlement logic
        let payment = self.calculate_payment(settlement)?;
        self.transfer_funds(&settlement.provider, payment)?;

        // CRITICAL: Emit receipt AFTER successful settlement
        self.pending_receipts.push(YourMarketReceipt {
            settlement_id: settlement_id.to_string(),
            provider: settlement.provider.clone(),
            units: settlement.units,
            payment_ct: payment,
            block_height: current_block,
            proof_data: settlement.proof_hash,
        });

        Ok(payment)
    }

    /// Drain all pending receipts for block inclusion
    pub fn drain_receipts(&mut self) -> Vec<YourMarketReceipt> {
        std::mem::take(&mut self.pending_receipts)
    }
}
```

**Pattern B: Market uses static modules (like Compute Settlement)**

```rust
// Inside your settlement module

static PENDING_RECEIPTS: Lazy<MutexT<Vec<YourMarketReceipt>>> =
    Lazy::new(|| mutex(Vec::new()));

pub fn settle(settlement_id: &str, current_block: u64) -> Result<(), Error> {
    // Settlement logic...

    // Emit receipt
    PENDING_RECEIPTS.guard().push(YourMarketReceipt {
        settlement_id: settlement_id.to_string(),
        // ... fields
    });

    Ok(())
}

pub fn drain_receipts() -> Vec<YourMarketReceipt> {
    std::mem::take(&mut *PENDING_RECEIPTS.guard())
}
```

### Phase 3: Global Accessor (Node-Side)

**File**: `node/src/rpc/your_market.rs` or `node/src/your_market/mod.rs`

Create a global market instance if it doesn't exist:

```rust
use concurrency::{Lazy, MutexT, mutex, MutexExt};
use your_market_crate::YourMarket;

static MARKET: Lazy<MutexT<YourMarket>> = Lazy::new(|| {
    let path = std::env::var("TB_YOUR_MARKET_DIR")
        .unwrap_or_else(|_| "your_market_data".to_string());
    let market = YourMarket::open(&path)
        .expect("failed to open your market");
    mutex(market)
});

/// Drain pending receipts for block inclusion
pub fn drain_your_market_receipts() -> Vec<crate::receipts::YourMarketReceipt> {
    MARKET
        .guard()
        .drain_receipts()
        .into_iter()
        .map(|r| crate::receipts::YourMarketReceipt {
            settlement_id: r.settlement_id,
            provider: r.provider,
            units: r.units,
            payment_ct: r.payment_ct,
            block_height: r.block_height,
            proof_data: r.proof_data,
        })
        .collect()
}
```

**Thread Safety Note**: Use `MutexT` (not `Arc`) to allow mutable access for draining.

### Phase 4: Block Construction Integration

**File**: `node/src/lib.rs` (around line 4563-4578)

Add your market to the receipt collection logic:

```rust
fn mine_block_with_ts(&mut self, miner_addr: &str, timestamp_millis: u64) -> PyResult<Block> {
    // ... existing block construction ...

    // Collect all market receipts
    let mut block_receipts: Vec<Receipt> = Vec::with_capacity(ad_settlements.len());

    // Ad market receipts
    for record in &ad_settlements {
        block_receipts.push(Receipt::Ad(AdReceipt { /* ... */ }));
    }

    // Energy market receipts
    for receipt in crate::energy::drain_energy_receipts() {
        block_receipts.push(Receipt::Energy(EnergyReceipt { /* ... */ }));
    }

    // Storage market receipts
    for receipt in crate::rpc::storage::drain_storage_receipts() {
        block_receipts.push(Receipt::Storage(receipt));
    }

    // Compute market receipts
    for receipt in crate::compute_market::drain_compute_receipts() {
        block_receipts.push(Receipt::Compute(receipt));
    }

    // ← ADD YOUR MARKET HERE
    for receipt in crate::your_market::drain_your_market_receipts() {
        block_receipts.push(Receipt::YourMarket(receipt));
    }

    // ... rest of block construction ...

    let block = Block {
        // ... all other fields ...
        receipts: block_receipts, // ← Receipts included in block
    };
}
```

### Phase 5: Telemetry Integration (Optional but Recommended)

**File**: `node/src/telemetry/receipts.rs`

Add metrics for your market:

```rust
pub static RECEIPTS_YOUR_MARKET: Lazy<Counter> = Lazy::new(|| {
    foundation_telemetry::register_counter!(
        "receipts_your_market_total",
        "Total your market receipts across all blocks"
    )
    .unwrap_or_else(|_| Counter::placeholder())
});

pub static RECEIPTS_YOUR_MARKET_PER_BLOCK: Lazy<IntGauge> = Lazy::new(|| {
    foundation_telemetry::register_int_gauge!(
        "receipts_your_market_per_block",
        "Number of your market receipts in current block"
    )
    .unwrap_or_else(|_| IntGauge::placeholder())
});

pub static RECEIPT_SETTLEMENT_YOUR_MARKET: Lazy<Gauge> = Lazy::new(|| {
    foundation_telemetry::register_gauge!(
        "receipt_settlement_your_market_ct",
        "Total your market receipt settlement (CT) in current block"
    )
    .unwrap_or_else(|_| Gauge::placeholder())
});
```

Update the `record_receipts` function:

```rust
pub fn record_receipts(receipts: &[Receipt], serialized_bytes: usize) {
    // ... existing counters ...

    let mut your_market_count = 0i64;
    let mut your_market_settlement = 0.0;

    for receipt in receipts {
        let settlement_ct = receipt.settlement_amount() as f64;
        match receipt {
            // ... existing cases ...
            Receipt::YourMarket(_) => {
                your_market_count += 1;
                your_market_settlement += settlement_ct;
                RECEIPTS_YOUR_MARKET.inc();
            }
        }
    }

    RECEIPTS_YOUR_MARKET_PER_BLOCK.set(your_market_count);
    RECEIPT_SETTLEMENT_YOUR_MARKET.set(your_market_settlement);
}
```

---

## Existing Market Implementations (Reference)

### Storage Market (Best Practice Example)

**Receipt Definition**: `node/src/receipts.rs:25-44`
```rust
pub struct StorageReceipt {
    pub contract_id: String,
    pub provider: String,
    pub bytes: u64,
    pub price_ct: u64,
    pub block_height: u64,
    pub provider_escrow: u64,
}
```

**Market-Side**: `storage_market/src/receipts.rs:10-41`
- Local `StorageSettlementReceipt` struct (avoids circular deps)
- `from_proof()` factory method - only creates receipt on success + payment
- Emitted in `storage_market/src/lib.rs` when `record_proof_outcome()` succeeds

**Global Accessor**: `node/src/rpc/storage.rs:586-600`
```rust
pub fn drain_storage_receipts() -> Vec<crate::receipts::StorageReceipt> {
    MARKET.guard().drain_receipts()
        .into_iter()
        .map(|r| crate::receipts::StorageReceipt {
            contract_id: r.contract_id,
            provider: r.provider,
            bytes: r.bytes,
            price_ct: r.price_ct,
            block_height: r.block_height,
            provider_escrow: r.provider_escrow,
        })
        .collect()
}
```

**Integration**: `node/src/lib.rs:4573-4575`

### Compute Market

**Receipt Definition**: `node/src/receipts.rs:46-65`

**Market-Side**: `node/src/compute_market/mod.rs:303,319-321,345-352`
- Receipts emitted in `sweep_overdue_jobs()` when job completes
- Contains job metadata + verification status

**Global Accessor**: `node/src/compute_market/mod.rs:808-820`
```rust
static COMPUTE_MARKET: Lazy<MutexT<Market>> =
    Lazy::new(|| mutex(Market::new()));

pub fn drain_compute_receipts() -> Vec<crate::ComputeReceipt> {
    COMPUTE_MARKET.guard().drain_receipts()
}
```

**Integration**: `node/src/lib.rs:4576-4578`

### Energy Market

**Receipt Definition**: `node/src/receipts.rs:67-86`

**Market-Side**: `crates/energy-market/src/lib.rs:74-83` (local struct)

**Global Accessor**: `node/src/energy.rs:380-389`
```rust
pub fn drain_energy_receipts() -> Vec<EnergyReceipt> {
    let mut guard = store();
    let receipts = guard.market.drain_receipts();
    if !receipts.is_empty() {
        if let Err(err) = guard.persist_market() {
            warn!(?err, "failed to persist energy market");
        }
    }
    receipts
}
```

**Integration**: `node/src/lib.rs:4563-4572`

### Ad Market

**Receipt Definition**: `node/src/receipts.rs:88-107`

**Market-Side**: Inline in block construction (no separate market crate)

**Integration**: `node/src/lib.rs:4554-4561`
- Receipts created directly from `pending_ad_settlements`

---

## Common Patterns & Gotchas

### Pattern: Local Receipt Struct (Avoid Circular Dependencies)

Markets in separate crates (storage, energy) define their own receipt struct to avoid depending on `node`:

```rust
// In market crate: storage_market/src/receipts.rs
pub struct StorageSettlementReceipt {
    pub contract_id: String,
    // ... same fields as canonical StorageReceipt
}

// In node: node/src/rpc/storage.rs
pub fn drain_storage_receipts() -> Vec<crate::receipts::StorageReceipt> {
    MARKET.guard().drain_receipts()
        .into_iter()
        .map(|r| crate::receipts::StorageReceipt {
            // Convert market receipt → canonical receipt
            contract_id: r.contract_id,
            // ...
        })
        .collect()
}
```

### Pattern: Thread-Safe Global Instance

```rust
// ✅ CORRECT - Allows mutable access
static MARKET: Lazy<MutexT<Market>> = Lazy::new(|| mutex(Market::new()));

// ❌ WRONG - Arc<T> doesn't allow mutation
static MARKET: Lazy<Arc<Market>> = Lazy::new(|| Arc::new(Market::new()));

// Access pattern
pub fn drain_receipts() -> Vec<Receipt> {
    MARKET.guard().drain_receipts() // .guard() gets MutexGuard
}
```

### Pattern: Emission Timing

```rust
// ✅ CORRECT - Emit AFTER successful settlement
pub fn settle(&mut self) -> Result<u64, Error> {
    self.transfer_funds()?; // May fail
    self.update_state()?;   // May fail

    // Only reaches here if settlement succeeded
    self.pending_receipts.push(Receipt { /* ... */ });
    Ok(payment)
}

// ❌ WRONG - Emit before settlement (can create receipt for failed tx)
pub fn settle(&mut self) -> Result<u64, Error> {
    self.pending_receipts.push(Receipt { /* ... */ });
    self.transfer_funds()?; // If this fails, receipt still exists!
    Ok(payment)
}
```

### Gotcha: Block Height Assignment

Receipts must know their block height at emission time:

```rust
// ✅ CORRECT - Pass current block height to settlement
pub fn settle(&mut self, current_block: u64) -> Result<(), Error> {
    self.pending_receipts.push(Receipt {
        block_height: current_block, // Known at emission
        // ...
    });
    Ok(())
}

// ❌ WRONG - Receipts don't know which block they're in
pub fn settle(&mut self) -> Result<(), Error> {
    self.pending_receipts.push(Receipt {
        block_height: 0, // Will be wrong!
        // ...
    });
    Ok(())
}
```

The block construction code must pass `index` (current block height) to market settlement functions.

### Gotcha: Receipt Deduplication

**Problem**: If block construction fails after draining receipts, they're lost.

**Current Solution**: Receipts are drained atomically during block construction. If the block fails to mine (e.g., difficulty too high), the block is retried with the **same receipts** because they weren't persisted yet.

**Important**: Markets should **not** persist receipt state separately. The source of truth is the blockchain.

---

## Testing Your Integration

### Unit Test: Receipt Emission

```rust
#[test]
fn receipt_emitted_on_successful_settlement() {
    let mut market = YourMarket::new();

    // Perform settlement
    market.settle("settlement_1", 100).unwrap();

    // Verify receipt was emitted
    let receipts = market.drain_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].settlement_id, "settlement_1");
    assert_eq!(receipts[0].block_height, 100);
}

#[test]
fn no_receipt_on_failed_settlement() {
    let mut market = YourMarket::new();

    // Attempt settlement that should fail
    let result = market.settle("invalid", 100);
    assert!(result.is_err());

    // Verify no receipt was emitted
    let receipts = market.drain_receipts();
    assert_eq!(receipts.len(), 0);
}

#[test]
fn drain_receipts_clears_pending() {
    let mut market = YourMarket::new();
    market.settle("s1", 100).unwrap();
    market.settle("s2", 100).unwrap();

    let receipts = market.drain_receipts();
    assert_eq!(receipts.len(), 2);

    // Second drain should be empty
    let receipts2 = market.drain_receipts();
    assert_eq!(receipts2.len(), 0);
}
```

### Integration Test: Block Inclusion

```rust
#[test]
fn receipts_included_in_mined_block() {
    let mut blockchain = Blockchain::new(/* ... */);

    // Trigger settlement in your market
    trigger_settlement(&mut blockchain, "settlement_1");

    // Mine a block
    let block = blockchain.mine_block("miner_addr").unwrap();

    // Verify receipt is in block
    let your_market_receipts: Vec<_> = block.receipts.iter()
        .filter_map(|r| match r {
            Receipt::YourMarket(r) => Some(r),
            _ => None,
        })
        .collect();

    assert_eq!(your_market_receipts.len(), 1);
    assert_eq!(your_market_receipts[0].settlement_id, "settlement_1");
}
```

### Telemetry Test

```rust
#[cfg(feature = "telemetry")]
#[test]
fn receipt_telemetry_recorded() {
    use crate::telemetry::receipts;

    let receipts = vec![
        Receipt::YourMarket(YourMarketReceipt {
            settlement_id: "s1".into(),
            payment_ct: 100,
            // ...
        }),
    ];

    receipts::record_receipts(&receipts, 256);

    // Verify metrics were updated
    // (actual verification depends on your telemetry framework)
}
```

---

## Debugging Receipt Issues

### Receipt Not Appearing in Blocks

**Check 1: Is the market settling?**
```rust
// Add logging to settlement function
pub fn settle(&mut self, id: &str, block: u64) -> Result<(), Error> {
    // ... settlement logic ...

    #[cfg(feature = "telemetry")]
    info!("Emitting receipt for settlement {}", id);

    self.pending_receipts.push(Receipt { /* ... */ });
    Ok(())
}
```

**Check 2: Is drain being called?**
```rust
// Add logging to drain function
pub fn drain_your_market_receipts() -> Vec<Receipt> {
    let receipts = MARKET.guard().drain_receipts();

    #[cfg(feature = "telemetry")]
    info!("Draining {} receipts from your market", receipts.len());

    receipts
}
```

**Check 3: Is block construction calling drain?**
```rust
// In node/src/lib.rs mine_block_with_ts
#[cfg(feature = "telemetry")]
info!("Block receipts before your market: {}", block_receipts.len());

for receipt in crate::your_market::drain_your_market_receipts() {
    block_receipts.push(Receipt::YourMarket(receipt));
}

#[cfg(feature = "telemetry")]
info!("Block receipts after your market: {}", block_receipts.len());
```

### Receipt Count Mismatch

**Symptom**: Telemetry shows N settlements but only M receipts in blocks.

**Cause**: Receipt emission is conditional (e.g., only on payment > 0).

**Solution**: Verify emission logic matches your expectations:

```rust
pub fn settle(&mut self, id: &str, block: u64) -> Result<u64, Error> {
    let payment = self.calculate_payment(id)?;

    // Only emit receipt if payment occurred
    if payment > 0 {
        self.pending_receipts.push(Receipt { /* ... */ });
    }

    Ok(payment)
}
```

### Double Counting

**Symptom**: Same settlement appears in multiple receipts.

**Cause**: `drain_receipts()` not using `std::mem::take` or equivalent.

**Solution**:

```rust
// ✅ CORRECT - Takes ownership, leaves empty vec
pub fn drain_receipts(&mut self) -> Vec<Receipt> {
    std::mem::take(&mut self.pending_receipts)
}

// ❌ WRONG - Clones, original remains
pub fn drain_receipts(&mut self) -> Vec<Receipt> {
    self.pending_receipts.clone() // Original still has receipts!
}
```

---

## Economics Derivation (Advanced)

Receipts enable **deterministic economics derivation** - computing market metrics purely from blockchain data.

**File**: `node/src/economics/deterministic_metrics.rs`

```rust
pub fn derive_market_metrics(blocks: &[Block]) -> MarketMetrics {
    let mut storage_volume = 0u64;
    let mut compute_volume = 0u64;
    let mut energy_volume = 0u64;

    for block in blocks {
        for receipt in &block.receipts {
            match receipt {
                Receipt::Storage(r) => {
                    storage_volume += r.bytes;
                }
                Receipt::Compute(r) => {
                    compute_volume += r.compute_units;
                }
                Receipt::Energy(r) => {
                    energy_volume += r.energy_units;
                }
                Receipt::YourMarket(r) => {
                    // Derive your market's metrics
                }
                _ => {}
            }
        }
    }

    MarketMetrics {
        storage_volume,
        compute_volume,
        energy_volume,
        // ...
    }
}
```

**Key Insight**: If you can compute a metric from receipts alone, it's **deterministic** and can be verified by light clients.

---

## Performance Considerations

### Receipt Vector Sizing

```rust
// Pre-allocate based on expected settlement count
let mut block_receipts: Vec<Receipt> = Vec::with_capacity(
    ad_settlements.len() +
    expected_storage_settlements +
    expected_compute_jobs +
    expected_energy_deliveries +
    expected_your_market_settlements
);
```

### Serialization Impact

Receipts increase block size. Monitor serialized size:

```rust
#[cfg(feature = "telemetry")]
{
    let serialized = crate::block_binary::encode_receipts(&block.receipts)
        .unwrap_or_default();
    crate::telemetry::receipts::record_receipts(&block.receipts, serialized.len());
}
```

**Typical sizes**:
- Storage: ~128 bytes per receipt
- Compute: ~96 bytes per receipt
- Energy: ~112 bytes per receipt
- Ad: ~80 bytes per receipt

**Block size limit**: Current implementation doesn't enforce a receipt-specific limit, but total block size is bounded.

### Mutex Contention

If your market has high settlement throughput, consider:

1. **Batch settlements** - Accumulate multiple settlements before emitting receipts
2. **Lock-free queues** - Use `crossbeam` channels instead of `Mutex<Vec<Receipt>>`
3. **Per-block draining** - Only drain once per block construction (already enforced)

---

## Security Considerations

### Receipt Authenticity

Receipts are **not cryptographically signed** by default. Trust model:

1. **Miners are trusted** - They include receipts in blocks they mine
2. **Consensus validates** - Invalid blocks (wrong receipts) are rejected
3. **Light clients verify** - Can check receipt inclusion via Merkle proofs

If you need **provable attribution** (e.g., provider signed receipt):

```rust
pub struct YourMarketReceipt {
    pub settlement_id: String,
    pub provider: String,
    pub signature: Vec<u8>, // Provider's signature over settlement
    // ...
}
```

### Prevent Receipt Forgery

Markets MUST validate settlements before emitting receipts:

```rust
pub fn settle(&mut self, proof: &Proof) -> Result<(), Error> {
    // ✅ Verify proof BEFORE emitting receipt
    if !self.verify_proof(proof)? {
        return Err(Error::InvalidProof);
    }

    // ✅ Check provider has sufficient balance
    if self.balance(&proof.provider) < payment {
        return Err(Error::InsufficientBalance);
    }

    // Only emit after all validations pass
    self.pending_receipts.push(Receipt { /* ... */ });
    Ok(())
}
```

### DOS via Receipt Spam

**Attack**: Malicious actor creates many small settlements to bloat receipts.

**Mitigation**:
1. **Minimum settlement size** - Require minimum payment for receipt emission
2. **Market admission control** - Rate limit settlements per provider
3. **Economic incentives** - Settlement costs exceed spam value

Example:

```rust
const MIN_PAYMENT_FOR_RECEIPT: u64 = 1000; // 0.001 CT

pub fn settle(&mut self, id: &str) -> Result<(), Error> {
    let payment = self.calculate_payment(id)?;

    // Only emit receipt for economically significant settlements
    if payment >= MIN_PAYMENT_FOR_RECEIPT {
        self.pending_receipts.push(Receipt { /* ... */ });
    }

    Ok(())
}
```

---

## Changelog Integration

When adding a new market's receipts, update:

1. **This file** (`INSTRUCTIONS.md`) - Add to "Existing Market Implementations"
2. **`docs/architecture.md`** - Document in receipts section
3. **`CHANGELOG.md`** - Note receipt system changes
4. **Tests** - Add integration tests in `node/tests/receipt_integration.rs`

---

## Quick Reference Card

```
┌─────────────────────────────────────────────────────────────┐
│ Receipt System Checklist                                    │
├─────────────────────────────────────────────────────────────┤
│ ☐ Define receipt struct in node/src/receipts.rs           │
│ ☐ Add variant to Receipt enum                              │
│ ☐ Implement market_name(), settlement_amount(), etc.    │
│ ☐ Create market-side receipt emission in settlement logic  │
│ ☐ Add pending_receipts: Vec<Receipt> to market struct      │
│ ☐ Implement drain_receipts() → Vec<Receipt>                │
│ ☐ Create global market instance with MutexT                │
│ ☐ Add drain_your_market_receipts() public function         │
│ ☐ Integrate in node/src/lib.rs mine_block_with_ts()        │
│ ☐ Add telemetry metrics in node/src/telemetry/receipts.rs  │
│ ☐ Write unit tests for emission logic                      │
│ ☐ Write integration test for block inclusion               │
│ ☐ Update documentation                                      │
│ ☐ cargo check --lib (must pass)                            │
│ ☐ cargo test --lib (receipt tests must pass)               │
└─────────────────────────────────────────────────────────────┘
```

---

## Contact & Support

**Questions?** Check:
1. Existing market implementations (storage, compute, energy, ad)
2. `node/src/receipts.rs` for canonical structures
3. `node/src/lib.rs:4500-4600` for block construction flow
4. `node/tests/receipt_integration.rs` for test patterns

**Found a bug?** File issue with:
- Market name
- Expected receipt count
- Actual receipt count (check telemetry)
- Block height where discrepancy occurred
- Settlement logs (if available)

---

**EOF** - You now understand the complete receipt system architecture. Go forth and emit receipts.
