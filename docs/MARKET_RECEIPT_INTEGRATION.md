# Market Receipt Emission Integration Guide

**Purpose:** Enable each market to emit receipts during settlement, feeding into consensus-level metrics.

---

## Architecture Overview

```
Market Settlement → Receipt Creation → Block.receipts → Hash → Metrics
```

**Key Principle:** Receipts are **append-only audit logs** created at settlement time, included in blocks, and hashed for consensus.

---

## Integration Pattern (All Markets)

### Step 1: Receipt Collection During Epoch

Each market needs an **epoch-level receipt buffer**:

```rust
// In your market's state struct (e.g., AdMarketState, StorageMarketState)
pub struct MarketState {
    // ... existing fields ...
    
    /// Receipts pending inclusion in next block
    pending_receipts: Vec<Receipt>,
}
```

### Step 2: Emit Receipt at Settlement

When a market transaction settles (payment processed, service delivered), create receipt:

```rust
use crate::receipts::{Receipt, AdReceipt, StorageReceipt, ComputeReceipt, EnergyReceipt};

// Example: Ad Market Settlement
fn settle_ad_campaign(
    &mut self,
    campaign_id: String,
    publisher: String,
    impressions: u64,
    spend_ct: u64,
    conversions: u32,
    block_height: u64,
) -> Result<(), SettlementError> {
    // 1. Process payment (existing logic)
    self.transfer_payment(&campaign_id, &publisher, spend_ct)?;
    
    // 2. Create receipt for audit trail
    let receipt = Receipt::Ad(AdReceipt {
        campaign_id: campaign_id.clone(),
        publisher: publisher.clone(),
        impressions,
        spend_ct,
        block_height,
        conversions,
    });
    
    // 3. Add to pending receipts
    self.pending_receipts.push(receipt);
    
    Ok(())
}
```

### Step 3: Flush Receipts to Block

When constructing a new block, collect all pending receipts:

```rust
// In block construction logic (likely in lib.rs or blockchain module)
impl Blockchain {
    fn construct_block(&mut self, miner_addr: String, nonce: u64) -> Result<Block, BlockError> {
        // ... existing block construction ...
        
        // Collect receipts from all markets
        let mut all_receipts = Vec::new();
        
        // Ad market receipts
        if let Some(ad_market) = &mut self.ad_market_state {
            all_receipts.extend(ad_market.pending_receipts.drain(..));
        }
        
        // Storage market receipts
        if let Some(storage_market) = &mut self.storage_market_state {
            all_receipts.extend(storage_market.pending_receipts.drain(..));
        }
        
        // Compute market receipts
        if let Some(compute_market) = &mut self.compute_market_state {
            all_receipts.extend(compute_market.pending_receipts.drain(..));
        }
        
        // Energy market receipts
        if let Some(energy_market) = &mut self.energy_market_state {
            all_receipts.extend(energy_market.pending_receipts.drain(..));
        }
        
        // Create block with receipts
        let block = Block {
            index: self.block_height + 1,
            // ... other fields ...
            receipts: all_receipts,
        };
        
        // Record telemetry
        #[cfg(feature = "telemetry")]
        {
            let serialized = crate::block_binary::encode_receipts(&block.receipts)
                .unwrap_or_default();
            crate::telemetry::receipts::record_receipts(&block.receipts, serialized.len());
        }
        
        Ok(block)
    }
}
```

---

## Market-Specific Integration

### Ad Market

**File:** Find where ad campaigns settle (likely `ad_market/src/settlement.rs` or similar)

**Receipt Fields:**
- `campaign_id`: Unique campaign identifier
- `publisher`: Publisher address receiving payment
- `impressions`: Number of ad impressions delivered
- `spend_ct`: Total CT tokens spent
- `conversions`: Number of conversions tracked
- `block_height`: Current block height

**When to Emit:**
- After ad impression delivery is verified
- Payment transferred to publisher
- Oracle confirms pricing

**Example Integration Point:**
```rust
// In ad settlement function
pub fn finalize_ad_delivery(
    state: &mut AdMarketState,
    campaign: &Campaign,
    delivery_proof: DeliveryProof,
    block_height: u64,
) -> Result<(), AdError> {
    // Existing settlement logic
    let spend = calculate_spend(&campaign, &delivery_proof)?;
    transfer_to_publisher(&campaign.publisher, spend)?;
    
    // NEW: Emit receipt
    state.pending_receipts.push(Receipt::Ad(AdReceipt {
        campaign_id: campaign.id.clone(),
        publisher: campaign.publisher.clone(),
        impressions: delivery_proof.impressions,
        spend_ct: spend,
        block_height,
        conversions: delivery_proof.conversions,
    }));
    
    Ok(())
}
```

---

### Storage Market

**File:** `node/src/storage/` or storage crate

**Receipt Fields:**
- `contract_id`: Storage contract identifier
- `provider`: Provider address
- `bytes`: Bytes stored this period
- `price_ct`: CT tokens paid
- `block_height`: Current block height
- `provider_escrow`: Escrow held for SLA

**When to Emit:**
- End of storage billing period
- Provider submits proof of storage
- Payment processed

**Example:**
```rust
pub fn settle_storage_period(
    contract: &StorageContract,
    proof: StorageProof,
    block_height: u64,
) -> Result<Receipt, StorageError> {
    // Verify proof
    verify_storage_proof(&proof)?;
    
    // Calculate payment
    let payment = contract.bytes * contract.price_per_byte;
    
    // Create receipt
    Ok(Receipt::Storage(StorageReceipt {
        contract_id: contract.id.clone(),
        provider: contract.provider.clone(),
        bytes: contract.bytes,
        price_ct: payment,
        block_height,
        provider_escrow: contract.escrow_amount,
    }))
}
```

---

### Compute Market

**File:** `node/src/compute_market/`

**Receipt Fields:**
- `job_id`: Compute job identifier
- `provider`: Provider address
- `compute_units`: Units of computation delivered
- `payment_ct`: CT tokens paid
- `block_height`: Current block height
- `verified`: Whether SNARK proof verified

**When to Emit:**
- Job completion with proof
- SNARK verification passes
- Payment released from escrow

**Example:**
```rust
pub fn complete_compute_job(
    job: &ComputeJob,
    result: ComputeResult,
    snark_proof: SnarkProof,
    block_height: u64,
) -> Result<Receipt, ComputeError> {
    // Verify SNARK
    let verified = verify_snark(&snark_proof, &job.circuit_hash)?;
    
    // Release payment
    if verified {
        release_escrow(&job.provider, job.payment)?;
    }
    
    // Create receipt
    Ok(Receipt::Compute(ComputeReceipt {
        job_id: job.id.clone(),
        provider: job.provider.clone(),
        compute_units: job.compute_units,
        payment_ct: job.payment,
        block_height,
        verified,
    }))
}
```

---

### Energy Market

**File:** `node/src/energy/`

**Receipt Fields:**
- `contract_id`: Energy delivery contract
- `provider`: Energy provider address
- `energy_units`: kWh or similar delivered
- `price_ct`: CT tokens paid per unit
- `block_height`: Current block height
- `proof_hash`: Hash of grid delivery proof

**When to Emit:**
- Grid operator confirms delivery
- Smart meter attestation verified
- Payment processed

**Example:**
```rust
pub fn settle_energy_delivery(
    contract: &EnergyContract,
    delivery: GridDelivery,
    attestation: SmartMeterAttestation,
    block_height: u64,
) -> Result<Receipt, EnergyError> {
    // Verify attestation
    verify_smart_meter_signature(&attestation)?;
    
    // Calculate payment
    let payment = delivery.energy_kwh * contract.price_per_kwh;
    
    // Hash proof for receipt
    let proof_hash = blake3::hash(&attestation.serialize()).into();
    
    // Create receipt
    Ok(Receipt::Energy(EnergyReceipt {
        contract_id: contract.id.clone(),
        provider: contract.provider.clone(),
        energy_units: delivery.energy_kwh,
        price_ct: payment,
        block_height,
        proof_hash,
    }))
}
```

---

## Testing Each Market

### Unit Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_receipt_emission_on_settlement() {
        let mut market = create_test_market();
        
        // Execute settlement
        let result = market.settle_transaction(
            test_campaign(),
            test_delivery_proof(),
            100, // block_height
        );
        
        assert!(result.is_ok());
        
        // Verify receipt was created
        assert_eq!(market.pending_receipts.len(), 1);
        
        let receipt = &market.pending_receipts[0];
        match receipt {
            Receipt::Ad(ad) => {
                assert_eq!(ad.campaign_id, "test_campaign");
                assert_eq!(ad.impressions, 1000);
                assert_eq!(ad.spend_ct, 500);
                assert_eq!(ad.block_height, 100);
            }
            _ => panic!("Expected Ad receipt"),
        }
    }
    
    #[test]
    fn test_receipt_flush_to_block() {
        let mut blockchain = create_test_blockchain();
        
        // Add receipts to markets
        blockchain.ad_market.pending_receipts.push(test_ad_receipt());
        blockchain.storage_market.pending_receipts.push(test_storage_receipt());
        
        // Mine block
        let block = blockchain.construct_block("miner".into(), 12345).unwrap();
        
        // Verify receipts included
        assert_eq!(block.receipts.len(), 2);
        
        // Verify pending receipts cleared
        assert_eq!(blockchain.ad_market.pending_receipts.len(), 0);
        assert_eq!(blockchain.storage_market.pending_receipts.len(), 0);
    }
}
```

---

## Integration Checklist

### Per Market

- [ ] Add `pending_receipts: Vec<Receipt>` to market state
- [ ] Find settlement function(s)
- [ ] Create receipt at settlement point
- [ ] Add receipt to pending buffer
- [ ] Write unit test for receipt emission
- [ ] Verify receipt serialization works

### Blockchain Level

- [ ] Collect receipts from all markets during block construction
- [ ] Clear pending_receipts after inclusion
- [ ] Serialize receipts for hashing
- [ ] Call telemetry::receipts::record_receipts()
- [ ] Verify blocks with receipts hash differently

### Validation

- [ ] Run: `cargo test --test receipt_integration`
- [ ] Check metrics: `curl localhost:9090/metrics | grep receipt`
- [ ] Mine test block with receipts
- [ ] Verify Launch Governor sees non-zero utilization
- [ ] Run economics replay: `cargo test replay_economics`

---

## Common Pitfalls

### ❌ Creating Receipts Without Settlement
```rust
// WRONG: Receipt without actual settlement
let receipt = Receipt::Ad(...);
self.pending_receipts.push(receipt);
// Missing: actual payment transfer!
```

### ✅ Receipt After Successful Settlement
```rust
// RIGHT: Receipt confirms completed settlement
transfer_payment(provider, amount)?; // Settlement first
let receipt = Receipt::Ad(...);      // Then receipt
self.pending_receipts.push(receipt);
```

### ❌ Forgetting to Clear Pending Receipts
```rust
// WRONG: Receipts accumulate forever
all_receipts.extend(market.pending_receipts.clone());
```

### ✅ Draining Pending Receipts
```rust
// RIGHT: Clear after inclusion
all_receipts.extend(market.pending_receipts.drain(..));
```

### ❌ Different Block Heights in Same Block
```rust
// WRONG: Receipts from different heights
Receipt::Ad(AdReceipt { block_height: 100, ... })  // In block 105?
```

### ✅ Consistent Block Height
```rust
// RIGHT: All receipts use current block height
let current_height = self.block_height + 1;
Receipt::Ad(AdReceipt { block_height: current_height, ... })
```

---

## Debugging Tips

### Check Receipt Creation
```rust
#[cfg(feature = "telemetry")]
eprintln!("Created receipt: market={} amount={}", 
    receipt.market_name(), 
    receipt.settlement_amount()
);
```

### Verify Receipt Inclusion
```rust
#[cfg(feature = "telemetry")]
info!("Block {} includes {} receipts: {:?}",
    block.index,
    block.receipts.len(),
    block.receipts.iter().map(|r| r.market_name()).collect::<Vec<_>>()
);
```

### Monitor Telemetry
```bash
# Watch receipt metrics in real-time
watch -n 1 'curl -s localhost:9090/metrics | grep -E "receipt|RECEIPT"'
```

---

## Performance Considerations

### Expected Load
- **Ad Market:** 100-500 receipts/block (high frequency, small campaigns)
- **Storage:** 10-50 receipts/block (periodic billing cycles)
- **Compute:** 20-100 receipts/block (batch job completions)
- **Energy:** 5-20 receipts/block (hourly or daily settlements)

**Total:** ~150-700 receipts/block

### Serialization Size
- **Per receipt:** ~100-300 bytes
- **Per block:** ~15-200 KB
- **Hash overhead:** Negligible (already hashing VDF proofs)

### Optimization
If receipts exceed 1000/block:
1. Batch multiple small settlements into one receipt
2. Use receipt compression for historical blocks
3. Consider off-chain receipt storage with Merkle root in block

---

## Next Steps

1. **Start with Ad Market** (most developed)
   - Find settlement code
   - Add pending_receipts buffer
   - Emit receipts
   - Test

2. **Add Telemetry Monitoring**
   - Deploy with telemetry enabled
   - Verify non-zero receipt counts
   - Check Launch Governor metrics

3. **Expand to Other Markets**
   - Storage → Compute → Energy
   - Follow same pattern
   - Validate metrics at each step

4. **End-to-End Validation**
   - Run full economics replay
   - Verify deterministic metrics
   - Test on testnet

---

**Questions or Issues?**

If you get stuck:
1. Check telemetry: `curl localhost:9090/metrics | grep receipt`
2. Run tests: `cargo test receipt`
3. Review RECEIPT_STATUS.md for architecture overview

**End of Integration Guide**
