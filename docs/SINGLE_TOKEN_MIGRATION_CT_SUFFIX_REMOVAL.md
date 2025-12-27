# Single Token Migration: CT Suffix Removal

## Overview

This document tracks the migration from a dual-token model (Consumer Token CT / Industrial Token IT) to a single BLOCK token model. The primary task is removing all `_ct` suffixes from field names throughout the codebase.

**Key Principles:**
- Drop `_ct` suffix entirely - do NOT replace with `_block`
- Consumer vs Industrial LANES remain (for traffic routing), but token model changes to single BLOCK token
- The `_it` fields (Industrial Token) have been removed entirely

---

## COMPLETED WORK

### 1. RolePayoutBreakdown Struct (explorer/src/lib.rs)

**Location:** `explorer/src/lib.rs` (around line 150-160)

**Changes made:**
```rust
// BEFORE:
pub struct RolePayoutBreakdown {
    pub total_ct: u64,
    pub viewer_ct: u64,
    pub host_ct: u64,
    pub hardware_ct: u64,
    pub verifier_ct: u64,
    pub liquidity_ct: u64,
    pub miner_ct: u64,
}

// AFTER:
pub struct RolePayoutBreakdown {
    pub total: u64,
    pub viewer: u64,
    pub host: u64,
    pub hardware: u64,
    pub verifier: u64,
    pub liquidity: u64,
    pub miner: u64,
}
```

**All usages updated in:**
- `explorer/src/lib.rs` - struct definition and all field accesses
- `cli/src/explorer.rs` - field accesses when printing output

---

### 2. SlaResolution Struct (node/src/compute_market/settlement.rs)

**Location:** `node/src/compute_market/settlement.rs` (around line 80-100)

**Changes made:**
```rust
// BEFORE:
pub struct SlaResolution {
    pub burned_ct: u64,
    pub refunded_ct: u64,
    // ...other fields
}

// AFTER:
pub struct SlaResolution {
    pub burned: u64,
    pub refunded: u64,
    // ...other fields
}
```

**All usages updated in:**
- `node/src/compute_market/settlement.rs` - struct definition
- `node/src/compute_market/mod.rs` - field usages
- `node/src/rpc/compute_market.rs` - JSON keys changed from "burned_ct"/"refunded_ct" to "burned"/"refunded"
- `cli/src/compute.rs` - JSON parsing updated

---

### 3. Block Struct (node/src/lib.rs)

**Location:** `node/src/lib.rs` (around line 598-720)

**Fields renamed (removed `_ct` suffix):**
```rust
// BEFORE → AFTER
storage_sub_ct → storage_sub
read_sub_ct → read_sub
read_sub_viewer_ct → read_sub_viewer
read_sub_host_ct → read_sub_host
read_sub_hardware_ct → read_sub_hardware
read_sub_verifier_ct → read_sub_verifier
read_sub_liquidity_ct → read_sub_liquidity
compute_sub_ct → compute_sub
proof_rebate_ct → proof_rebate
```

**IMPORTANT:** The Block struct retains serde aliases for backward compatibility with stored block data:
```rust
#[serde(default = "...", alias = "storage_sub_ct")]
pub storage_sub: TokenAmount,
```

This allows reading old blocks from the database while using new field names in code.

**All usages updated in:**
- `node/src/lib.rs` - struct definition and all field accesses throughout the file
- `node/src/block_binary.rs` - encoder now writes new key names, decoder accepts both old and new keys
- `node/src/light_client/proof_tracker.rs` - updated `.proof_rebate` access

---

### 4. Block Binary Encoder/Decoder (node/src/block_binary.rs)

**Encoder (around line 150-200):** Now writes fields with new names (e.g., "storage_sub" not "storage_sub_ct")

**Decoder (around line 300-400):** Accepts both old and new key names for backward compat:
```rust
"storage_sub" | "storage_sub_ct" => assign_once(&mut storage_sub, reader.read_u64()?, "storage_sub"),
"read_sub" | "read_sub_ct" => assign_once(&mut read_sub, reader.read_u64()?, "read_sub"),
// etc...
```

**Test code (around line 750-830):** Updated to use new field names in sample_block() construction.

---

### 5. Tests Updated

**tests/partition_recovery.rs:**
- Updated `dummy_block()` function to use new field names
- Removed `_it` fields entirely
- Added missing ad_* fields
- Changed integer literals to `.into()` for TokenAmount conversion

---

## REMAINING WORK

### 1. explorer/tests/block_api.rs

**Status:** PARTIALLY DONE - needs completion

**Lines to update:**

**Lines 44-61:** JSON fixture - change keys from `_ct` suffix to no suffix:
```json
// BEFORE:
"storage_sub_ct": 0,
"read_sub_ct": {read_total},
"read_sub_viewer_ct": {read_viewer},
...
"compute_sub_ct": 0,
"proof_rebate_ct": 0,

// AFTER:
"storage_sub": 0,
"read_sub": {read_total},
"read_sub_viewer": {read_viewer},
...
"compute_sub": 0,
"proof_rebate": 0,
```

**Lines 142-159:** Same JSON fixture changes for the second test block.

**Lines 212:** JSON fixture - `"read_sub_ct"` → `"read_sub"`

**Lines 277-282:** Block struct construction - change field names:
```rust
// BEFORE:
read_sub_ct: TokenAmount::new(read_total),
read_sub_viewer_ct: TokenAmount::new(read_viewer),
...

// AFTER:
read_sub: TokenAmount::new(read_total),
read_sub_viewer: TokenAmount::new(read_viewer),
...
```

**Lines 306:** JSON fixture - `"read_sub_ct"` → `"read_sub"`

---

### 2. Governance Params (governance/src/params.rs AND node/src/governance/params.rs)

**Status:** NOT STARTED

**Fields to rename (both files have identical structures):**

```rust
// Struct fields (around line 569 in governance/, line 287 in node/):
pub beta_storage_sub_ct: i64,   → pub beta_storage_sub: i64,
pub gamma_read_sub_ct: i64,     → pub gamma_read_sub: i64,
pub kappa_cpu_sub_ct: i64,      → pub kappa_cpu_sub: i64,
pub lambda_bytes_out_sub_ct: i64, → pub lambda_bytes_out_sub: i64,
pub treasury_percent_ct: i64,   → pub treasury_percent: i64,
pub proof_rebate_limit_ct: i64, → pub proof_rebate_limit: i64,
pub rent_rate_ct_per_byte: i64, → pub rent_rate_per_byte: i64,
```

**Files affected:**
- `governance/src/params.rs` - struct definition, Default impl, to_json(), from_json(), apply functions
- `node/src/governance/params.rs` - same changes
- `governance/src/store.rs` - ParamKey enum and param key matching
- `node/src/governance/store.rs` - same changes
- `node/src/rpc/governance.rs` - GovernanceParams response struct
- `node/src/rpc/client.rs` - GovernanceParams parsing
- `node/src/rpc/inflation.rs` - InflationParams struct
- `node/src/ledger_binary.rs` - binary encoding/decoding of params
- `node/src/lib.rs` - `beta_storage_sub_ct_raw`, `gamma_read_sub_ct_raw` fields
- `node/src/config.rs` - InflationConfig struct
- `node/tests/subsidy_smoothing.rs` - test assertions
- `node/tests/inflation_params_rpc.rs` - test assertions
- `node/tests/ad_read_distribution.rs` - setting `gamma_read_sub_ct_raw`

**Note:** The ParamKey enum likely has variants like `BetaStorageSubCt`, `GammaReadSubCt` that need renaming.

---

### 3. DNS Gateway Fields (node/src/gateway/dns.rs)

**Status:** NOT STARTED

**Fields to rename (approximately 50+ occurrences):**

```rust
// Config struct (around line 69):
base_reserve_ct: u64,  → base_reserve: u64,

// Bid struct (around line 169):
stake_locked_ct: u64,  → stake_locked: u64,

// Auction record structs (around line 180-210):
min_bid_ct: u64,           → min_bid: u64,
stake_requirement_ct: u64, → stake_requirement: u64,
last_sale_price_ct: u64,   → last_sale_price: u64,
price_ct: u64,             → price: u64,
protocol_fee_ct: u64,      → protocol_fee: u64,
royalty_fee_ct: u64,       → royalty_fee: u64,
settlement_ct: u64,        → settlement: u64,
locked_ct: u64,            → locked: u64,
deposit_ct: u64,           → deposit: u64,
withdraw_ct: u64,          → withdraw: u64,
available_ct,              → available,
withdrawn_ct,              → withdrawn,
```

**Also update:**
- All function parameters using these names
- All JSON key strings in to_json() functions
- All JSON key lookups in from_json() functions
- All metric recording functions
- settlement_amounts_ct Vec field

---

### 4. Storage Contract Fields (storage/src/contract.rs)

**Status:** NOT STARTED

**Fields to rename:**
```rust
// Line 31:
pub total_deposit_ct: u64, → pub total_deposit: u64,
```

**Files affected:**
- `storage/src/contract.rs` - struct definition
- `storage/src/provider_integration.rs` - field usage
- `storage/tests/proof_security.rs` - test struct construction
- `storage/tests/market_incentives.rs` - test struct construction
- `storage_market/tests/importer.rs` - JSON serialization
- `storage_market/tests/engine_paths.rs` - field assertions (total_deposit_ct, amount_accrued_ct, remaining_deposit_ct, slashed_ct, deposit_ct)
- `tests/storage_market.rs` - test struct construction

---

### 5. DEX AMM Functions (dex/src/amm.rs)

**Status:** NOT STARTED

**This relates to the dual-token model removal. Functions to rename:**

```rust
// Line 52:
pub fn swap_ct_for_it(&mut self, ct_in: u128) -> u128
→ Consider removing entirely or renaming to single-token swap

// Line 63:
pub fn swap_it_for_ct(&mut self, it_in: u128) -> u128
→ Consider removing entirely or renaming to single-token swap
```

**Internal variables to rename:**
- `share_ct` → `share`
- `new_ct` → `new_reserve` (or similar)
- `ct_out` → `out` (or similar)

**Files affected:**
- `dex/src/amm.rs` - function definitions
- `dex/tests/amm.rs` - test calls
- `sim/dex_liquidity.rs` - simulation using swap functions

**QUESTION FOR USER:** Should these DEX swap functions be removed entirely as part of dual-token removal, or just renamed?

---

### 6. Misc Test Files

**Status:** NOT STARTED

**sim/fee_spike.rs line 17:**
```rust
pct_ct: 100,  → pct: 100,  (or pct_block if needed)
```

**sim/mempool_spam.rs line 26:**
```rust
pct_ct: 100,  → pct: 100,
```

**tests/base_fee_adjustment.rs line 12:**
```rust
pct_ct: 100,  → pct: 100,
```

**tests/shard_consensus.rs lines 20, 35:**
```rust
pct_ct: 0,  → pct: 0,
```

**tests/account_abstraction.rs line 38:**
```rust
pct_ct: 100,  → pct: 100,
```

---

### 7. Governance Treasury (governance/src/treasury.rs)

**Status:** NOT STARTED

**Lines 287-289:**
```rust
required_ct: u64, → required: u64,
available_ct: u64, → available: u64,
```

---

### 8. Metrics Aggregator (metrics-aggregator/src/lib.rs)

**Status:** NOT STARTED

**Line 8283:**
```rust
.or_else(|| obj.get("delta_ct"))  → .or_else(|| obj.get("delta"))
```

Also check for any `delta_ct` JSON key references.

---

### 9. Metrics Treasury Test (metrics-aggregator/tests/treasury.rs)

**Status:** NOT STARTED

**Line 143:**
```json
"delta_ct": "450",  → "delta": "450",
```

---

## VERIFICATION STEPS

After all changes:

1. `cargo check` - ensure compilation
2. `cargo test` - run all tests
3. Search for remaining `_ct` references: `grep -r "_ct[^a-z]" --include="*.rs" .`
4. Search for remaining `_it` references: `grep -r "_it[^a-z]" --include="*.rs" .`

---

## SERDE ALIAS STRATEGY

For backward compatibility with stored data, certain structs use serde aliases:

**Block struct:** Uses aliases to read old block data from database:
```rust
#[serde(alias = "storage_sub_ct")]
pub storage_sub: TokenAmount,
```

**Binary decoder:** Accepts both old and new key names:
```rust
"storage_sub" | "storage_sub_ct" => ...
```

**JSON fixtures in tests:** Should be updated to use new key names, but serde aliases ensure old data still works.

---

## FILES SUMMARY

### Completed:
- `node/src/lib.rs`
- `node/src/block_binary.rs`
- `node/src/compute_market/settlement.rs`
- `node/src/compute_market/mod.rs`
- `node/src/rpc/compute_market.rs`
- `node/src/light_client/proof_tracker.rs`
- `explorer/src/lib.rs`
- `cli/src/explorer.rs`
- `cli/src/compute.rs`
- `tests/partition_recovery.rs`

### Remaining:
- `explorer/tests/block_api.rs` (partial)
- `governance/src/params.rs`
- `governance/src/store.rs`
- `governance/src/treasury.rs`
- `node/src/governance/params.rs`
- `node/src/governance/store.rs`
- `node/src/rpc/governance.rs`
- `node/src/rpc/client.rs`
- `node/src/rpc/inflation.rs`
- `node/src/ledger_binary.rs`
- `node/src/config.rs`
- `node/src/gateway/dns.rs`
- `node/tests/subsidy_smoothing.rs`
- `node/tests/inflation_params_rpc.rs`
- `node/tests/ad_read_distribution.rs`
- `storage/src/contract.rs`
- `storage/src/provider_integration.rs`
- `storage/tests/proof_security.rs`
- `storage/tests/market_incentives.rs`
- `storage_market/tests/importer.rs`
- `storage_market/tests/engine_paths.rs`
- `tests/storage_market.rs`
- `dex/src/amm.rs`
- `dex/tests/amm.rs`
- `sim/dex_liquidity.rs`
- `sim/fee_spike.rs`
- `sim/mempool_spam.rs`
- `tests/base_fee_adjustment.rs`
- `tests/shard_consensus.rs`
- `tests/account_abstraction.rs`
- `metrics-aggregator/src/lib.rs`
- `metrics-aggregator/tests/treasury.rs`

---

## GREP COMMANDS FOR FINDING REMAINING ISSUES

```bash
# Find all _ct suffixed field/variable names
grep -rn "_ct[^a-z]" --include="*.rs" .

# Find specific patterns
grep -rn "storage_sub_ct" --include="*.rs" .
grep -rn "read_sub_ct" --include="*.rs" .
grep -rn "compute_sub_ct" --include="*.rs" .
grep -rn "proof_rebate_ct" --include="*.rs" .
grep -rn "burned_ct" --include="*.rs" .
grep -rn "refunded_ct" --include="*.rs" .

# Find IT token references (should be removed)
grep -rn "_it[^a-z]" --include="*.rs" .
```

---

*Last updated: 2025-12-27*
*Current status: Block struct and core structs updated, governance params and DNS gateway pending*
