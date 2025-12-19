# Manual Step: Integrate Receipts into Block Hashing

## Problem

The `BlockEncoder` struct now has `receipts_serialized: &'a [u8]` field added and the hashing logic includes receipts. However, **call sites must be updated** to pass serialized receipt data.

## Solution

### Step 1: Find BlockEncoder Usage

Searchin `node/src/lib.rs`:
```bash
grep -n "hashlayout::BlockEncoder" node/src/lib.rs
```

Look for patterns like:
```rust
let enc = crate::hashlayout::BlockEncoder { ... };
```

### Step 2: Serialize Receipts Before Hash

Wherever `BlockEncoder` is instantiated, add this:

```rust
// Serialize receipts to bytes for consensus-critical hashing
let receipts_bytes = match block_binary::encode_receipts(&block.receipts) {
    Ok(bytes) => bytes,
    Err(e) => {
        eprintln!("Receipt serialization failed: {}", e);
        vec![]
    }
};
```

### Step 3: Pass to BlockEncoder

Update the `BlockEncoder` instantiation:
```rust
let enc = crate::hashlayout::BlockEncoder {
    // ... existing fields ...
    receipts_serialized: &receipts_bytes,  // ADD THIS LINE
};
```

### Step 4: Create Helper in block_binary.rs

Add this function to `node/src/block_binary.rs`:

```rust
/// Serialize receipts to bytes for block hashing (consensus-critical)
pub fn encode_receipts(receipts: &[Receipt]) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(1024);
    write_receipts(&mut writer, receipts)?;
    Ok(writer.finish())
}
```

## Testing

After integration, verify:

```bash
# Should compile
cargo build --lib

# Should pass hash tests
cargo test --lib hash

# Verify determinism: same receipts → same hash
cargo test block_round_trip --lib
```

## Verification

Create a test to verify receipt hash inclusion:

```rust
#[test]
fn block_hash_changes_with_different_receipts() {
    let block1 = create_block_with_receipt(Receipt::Ad(...));
    let block2 = create_block_with_receipt(Receipt::Storage(...));
    
    let hash1 = calculate_hash(&block1);
    let hash2 = calculate_hash(&block2);
    
    assert_ne!(hash1, hash2, "Different receipts must produce different hashes");
}
```

## Critical

⚠️ **This is consensus-breaking:** All nodes must deploy simultaneously. Blocks mined before this change will have incorrect hashes on updated nodes.

**Deployment Strategy:**
1. Deploy hash integration to all nodes
2. Verify all nodes on same chain
3. Only then start emitting receipts (markets update)
