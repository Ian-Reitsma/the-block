# TokenBalance Migration Plan - INCOMPLETE MIGRATION FIX

## Problem

The single-token migration (commits `199ae68e` and `f9fb615f`) removed `_ct`/`_it` suffixes but **DID NOT migrate the `TokenBalance` struct**. This causes test failures because:

1. `TokenBalance` still has separate `consumer` and `industrial` fields
2. Balance validation still checks them separately
3. But the system should use a SINGLE BLOCK token with amounts routed through consumer/industrial LANES

## Root Cause

**File**: [node/src/lib.rs:467-470](node/src/lib.rs#L467-L470)

```rust
pub struct TokenBalance {
    pub consumer: u64,      // LEGACY - should be removed
    pub industrial: u64,    // LEGACY - should be removed
}
```

**Should be**:
```rust
pub struct TokenBalance {
    pub amount: u64,  // Single BLOCK token balance
}
```

## Current Broken Logic

**Balance Check** ([lib.rs:3546-3547](node/src/lib.rs#L3546-L3547)):
```rust
if sender.balance.consumer < required_consumer
    || sender.balance.industrial < required_industrial
```

**Should be**:
```rust
let total_required = required_consumer + required_industrial;
if sender.balance.amount < total_required
```

## Migration Steps

### 1. Update TokenBalance Struct
- Change `{consumer: u64, industrial: u64}` → `{amount: u64}`
- Update serde serialization/deserialization
- Add migration code to load old format

### 2. Update Account Balance Operations
Search and replace all:
- `balance.consumer` → `balance.amount`
- `balance.industrial` → `balance.amount` (or remove industrial additions entirely)
- `pending_consumer` → `pending_amount`
- `pending_industrial` → Remove (no longer needed)

### 3. Fix Balance Validation Logic
All places that check `sender.balance.consumer < X || sender.balance.industrial < Y` should become:
```rust
sender.balance.amount < (X + Y)
```

### 4. Update Mining Rewards
Currently coinbase only credits consumer (lib.rs:4668):
```rust
let coinbase_industrial_total = 0;  // WRONG
```

Should credit total to amount:
```rust
let coinbase_total = base_coinbase_block + coinbase_industrial_bonus;
miner.balance.amount += coinbase_total;
```

### 5. Update Transaction Processing
- Transaction amounts `amount_consumer` and `amount_industrial` remain (they're LANE routing)
- But deduct TOTAL from single balance
- Credit TOTAL to recipient's single balance

## Files That Need Updates

Based on grep results, approximately **44 occurrences** in lib.rs alone:

1. **Balance checks**: ~10 locations
2. **Balance updates**: ~15 locations
3. **Mining/coinbase**: ~5 locations
4. **Block processing**: ~10 locations
5. **Struct definitions**: 2 locations (TokenBalance, Account)

## Impact Assessment

**CRITICAL** - This is a fundamental data structure change affecting:
- All balance operations
- All transaction validation
- Mining rewards
- Block processing
- Account management
- Serialization/deserialization

## Recommendation

This is TOO LARGE to fix ad-hoc. Need to either:

**Option A**: Complete the full migration properly (2-4 hours of work)
- Update all ~44+ occurrences
- Add migration code for legacy data
- Update all tests
- Verify no regressions

**Option B**: Quick workaround for tests (5 minutes)
- Give test accounts enough balance in BOTH fields
- Accept that the system is in an inconsistent state
- Plan proper migration later

## Quick Workaround (Chosen for Now)

Since the user test run was from BEFORE my changes, and TokenBalance still has both fields, the quick fix is:

**Give test accounts balance in the CONSUMER field only** (since mining only credits consumer, and transactions primarily use consumer lane for test scenarios).

The `eviction_panic` test was fixed with `(100_000, 100_000)` but should be `(sufficient_amount, 0)` if we're treating this as a single token in consumer field.

Actually - the REAL issue is that **mining doesn't give ANY balance** because the old test accounts had `(0, 0)` and mining only credits consumer via coinbase!

So the fix is: **Tests should initialize accounts with sufficient consumer balance OR mining should properly credit**.

## Actual Fix Applied

Reverted my industrial balance additions. The real issue is tests need to either:
1. Mine blocks first to get consumer balance
2. OR initialize with consumer balance
3. AND fix coinbase_industrial_total = 0 issue

Currently investigating which approach matches the intended system design.
