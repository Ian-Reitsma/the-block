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

**IMPORTANT:** Legacy serde aliases have been removed. Nodes now emit and accept only the new field names, so historical block data must be migrated (or re-ingested) to keep parity.

**All usages updated in:**
- `node/src/lib.rs` - struct definition and all field accesses throughout the file
- `node/src/block_binary.rs` - encoder/decoder both use the new key names exclusively
- `node/src/light_client/proof_tracker.rs` - updated `.proof_rebate` access

---

### 4. Block Binary Encoder/Decoder (node/src/block_binary.rs)

**Encoder (around line 150-200):** Now writes fields with new names (e.g., "storage_sub" not "storage_sub_ct")

**Decoder (around line 300-400):** Only recognizes the new key names; the `_ct` aliases were removed to keep the binary codec consistent with the single-token spec.

**Test code (around line 750-830):** Updated to use new field names in sample_block() construction.

---

### 5. Tests Updated

**tests/partition_recovery.rs:**
- Updated `dummy_block()` function to use new field names
- Removed `_it` fields entirely
- Added missing ad_* fields
- Changed integer literals to `.into()` for TokenAmount conversion


### 6. Governance Subsidy + Treasury Params

**Locations:** `governance/src/{lib.rs,params.rs,codec.rs,store.rs,treasury.rs}`, `node/src/governance/{mod.rs,params.rs,codec.rs,store.rs}`, `node/src/{lib.rs,ledger_binary.rs,rpc/{governance.rs,inflation.rs},rpc/client.rs,config.rs}`, `cli/src/gov.rs`, `config/inflation.toml`, `node/tests/{subsidy_smoothing.rs,inflation_params_rpc.rs,rpc_inflation.rs,treasury.rs,ad_read_distribution.rs}`, `docs/system_reference.md`

- Dropped `_ct` suffixes from the subsidy multipliers and treasury knobs:
  - `beta_storage_sub`, `gamma_read_sub`, `kappa_cpu_sub`, `lambda_bytes_out_sub`
  - `treasury_percent`, `proof_rebate_limit`, `rent_rate_per_byte`
- Renamed the raw economics samples on `Blockchain` (`beta_storage_sub_raw`, etc.) and updated the deterministic metrics plumbing.
- `ParamKey` variants, codecs, sled stores, RPC payloads, config defaults, and ledger binary encoding now emit the new identifiers. RPC clients and tests expect the updated JSON keys.
- `DisbursementError::InsufficientFunds` now reports `{required, available}` for the single BLOCK balance; the `_ct` suffix is gone.
- Docs now describe the new identifiers, keeping the spec aligned with implementation.

---

### 7. Explorer Block Payout Fixtures (cli/tests/explorer.rs)

- Block payout fixtures now serialize `storage_sub`, `read_sub_*`, `compute_sub`, and `proof_rebate` without `_ct`. Table/Prometheus outputs, JSON parsing, and the CLI tests all exercise the new field names.

---

### 8. Storage Contract + Importer Totals (storage/**, node/src/rpc/storage.rs, cli/tests/storage_importer.rs)

- `StorageContract::total_deposit` replaces the old `_ct` field everywhere (storage market codec, importer, RPC, CLI, integration tests). JSON keys changed so snapshots/imports never emit `_ct`.

---

### 9. DNS Gateway Auction Fields (node/src/gateway/dns.rs, cli/src/gateway.rs, node/tests/dns_auction_ledger.rs, node/src/launch_governor/mod.rs)

- DNS config, bid/auction records, CLI payloads, and telemetry now expose `base_reserve`, `min_bid`, `stake_requirement`, `stake_locked`, `deposit`, `bid`, `price`, `protocol_fee`, `royalty_fee`, `settlement_amounts`, `locked`, `withdraw`, and `coverage_demand` without `_ct`.
- Launch Governor samples track `settlement_p90` and compute coverage using the renamed settlement vector so metrics stay consistent with the runtime structs.

### 10. DEX AMM Pool + Sims

- `dex/src/amm.rs` drops the dual-token naming. The pool now exposes `base_reserve`/`quote_reserve` and swaps via `swap_base_for_quote` and `swap_quote_for_base`, so the AMM math no longer references `_ct`/`_it`.
- `dex/tests/amm.rs` and `sim/dex_liquidity.rs` exercise the renamed helpers and verify slippage/invariant guarantees with the single-token naming.
- `node/src/dex/storage_binary.rs` encodes/decodes AMM pools with the new field names, and the sled round-trip tests cover the updated schema.

### 11. Metrics Aggregator Treasury History

- `metrics-aggregator/src/lib.rs` now expects the `delta` field when parsing treasury balance snapshots; the legacy `delta_ct` alias was removed to keep telemetry aligned with the single-token schema.
- `metrics-aggregator/tests/treasury.rs` exercises the updated schema by feeding legacy string payloads that emit `delta`.
- `docs/RECEIPT_STATUS.md` and `docs/RECEIPT_INTEGRATION_COMPLETE.md` document the new field requirement so operators update their CLI dumps and dashboards.

### 12. Receipt Settlement Fields

- `StorageReceipt`, `EnergyReceipt`, `ComputeReceipt`, and `AdReceipt` drop the `_ct` suffixes (`price`, `payment`, `spend` now express settlement amounts in BLOCK). The binary codec, crypto hashing, validation logic, deterministic metrics engine, and compute/storage RPC drainers were updated accordingly.
- All receipt integration tests, stress tests, and security suites now use the new field names; telemetry + deterministic derivation consume the updated identifiers.
- Receipt docs (`RECEIPT_STATUS.md`, `RECEIPT_INTEGRATION_COMPLETE.md`, `MARKET_RECEIPT_INTEGRATION.md`, `RECEIPT_VALIDATION_GUIDE.md`, `INSTRUCTIONS.md`, and the architecture spec) describe the renamed fields so operators and tooling stop emitting `_ct` payloads.

### 13. Transaction Fee Split Surfaces (`pct`)

- `RawTxPayload::pct` replaces the legacy `pct_ct` flag throughout the ledger, network envelopes, RPCs, CLI/wallet tooling, and the binary codec (`node/src/transaction.rs`, `node/src/transaction/binary.rs`, `node/src/net/message.rs`, `cli/src/wallet.rs`, `cli/src/tx.rs`, `scripts/node_e2e.sh`, `scripts/node_drive_existing.sh`). All mempool/fee tests (`tests/base_fee_adjustment.rs`, `node/tests/mempool_*`, python fixtures) now emit the new key, ensuring blocks serialize the updated field name exclusively.
- Fee decomposition helpers and compute-market admission structs now expose `pct`/`fee_pct` and return `(fee_consumer, fee_industrial)` tuples, keeping the naming aligned with the single-token spec.

### 14. Storage Contract Deposits

- Storage market records, codecs, RPC payloads, and CLI/importer fixtures now use `deposit`/`remaining_deposit` instead of the `_ct` suffixed variants (`storage_market/src/lib.rs`, `storage_market/src/codec.rs`, `storage_market/src/receipts.rs`, `node/src/rpc/storage.rs`, `storage/tests/market_incentives.rs`, `storage_market/tests/*`, `cli/tests/storage_importer.rs`). Docs describing replica incentives mirror the new identifiers.

### 15. Fee Vector & Schema Updates

- `docs/spec/fee_v2.schema.json` (`mdbook` copy included) and `node/tests/vectors/fee_v2_vectors.csv` now publish `fee_consumer`/`fee_industrial`, and the CSV-driven tests (`node/tests/test_fee_vectors.py`, `node/tests/fee_vectors.rs`) assert against those keys so tooling stops referencing `_ct`/`_it`.

### 16. Energy Treasury Telemetry

- The treasury fee counter now emits as `energy_treasury_fee_total` in telemetry (`node/src/telemetry.rs`, `node/src/energy.rs`) and the associated docs (`docs/architecture.md`, `docs/developer_handbook.md`), keeping dashboards/operators on the BLOCK-denominated label without the `_ct` suffix.

### 17. Compute Settlement Ledger

- `node/src/compute_market/settlement.rs` renamed the ledger key from `ledger_ct` to `ledger`, with legacy migration for old keys.
- `AuditRecord` struct now uses `delta: i64` instead of `delta_ct: i64` / `delta_it: Option<i64>`.
- Internal struct field `ct: AccountLedger` renamed to `ledger: AccountLedger`.
- `record_event` function signature simplified to 3 parameters (removed `delta_it`).
- `accrue_split` and `refund_split` parameter names changed from `ct`/`it` to `consumer`/`industrial` (lane-based).
- All usages updated in `node/src/rpc/compute_market.rs` (JSON keys) and test files.

### 18. Telemetry Metric Renames

- `BASE_REWARD_CT` → `BASE_REWARD`, metric name `base_reward_ct` → `base_reward`
- `DNS_AUCTION_SETTLEMENT_CT` → `DNS_AUCTION_SETTLEMENT`, metric name `dns_auction_settlement_ct` → `dns_auction_settlement`
- `DNS_STAKE_LOCKED_CT` → `DNS_STAKE_LOCKED`, metric name `dns_stake_locked_ct` → `dns_stake_locked`
- Receipt settlement metrics renamed: `receipt_settlement_*_ct` → `receipt_settlement_*` for storage, compute, energy, ad markets
- Function parameters renamed: `settlement_ct` → `settlement`, `delta_ct` → `delta`

### 19. Governance Treasury Error

- `DisbursementError::InsufficientFunds` simplified from `{required, required_it, available, available_it}` to `{required, available}` for single BLOCK balance.

### 20. Test File Updates

- `node/tests/compute_settlement.rs` - Updated `contains_entry` helper and assertions for simplified `AuditRecord`
- `node/tests/compute_market_fee_split.rs` - Renamed `bal_ct`/`bal_it` to `bal_consumer`/`bal_industrial` for clarity

---

## REMAINING WORK

### 1. mdBook Artifacts

- Rebuild/publish the mdBook artifacts (`docs/book/**/*.html`, search index) so the generated pages pick up the new subsidy/metric names.

### Outstanding Files
- `docs/book/system_reference.html`, `docs/book/print.html`, `docs/book/searchindex.js` (may contain stale `_ct` names until rebuilt)

---

## LEGITIMATE LEGACY REFERENCES

The following `_ct`/`_it` references in the codebase are intentional migration code and should NOT be removed:

1. **SQL Migrations** (`explorer/src/lib.rs`, `explorer/src/bin/migrate_treasury_db.rs`):
   - `RENAME COLUMN amount_ct TO amount` - renaming old column to new name
   - `DROP COLUMN amount_it` - removing deprecated column

2. **Legacy Key Migration** (`node/src/compute_market/settlement.rs`):
   - `KEY_LEDGER_LEGACY_CT` and `KEY_LEDGER_LEGACY_IT` - constants for loading old database keys
   - `legacy_ct` and `legacy_it` local variables - only used to migrate old data into new format

---

## GREP COMMANDS FOR FINDING REMAINING ISSUES

```bash
# Find all _ct suffixed field/variable names (should only show migration code)
grep -rn "_ct[^a-z]" --include="*.rs" .

# Find IT token references (should only show migration code)
grep -rn "_it[^a-z]" --include="*.rs" .
```

---

*Last updated: 2025-12-27*
*Current status: Rust codebase migration COMPLETE. All `_ct`/`_it` suffixes removed from active code. Only migration/legacy loading code retains these references.*
