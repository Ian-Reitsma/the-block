# Energy Market RPC Endpoints

**Status**: Production-grade oracle and settlement interface  
**Location**: `node/src/rpc/energy.rs`, `crates/oracle-adapter/`  
**Canonical Format**: JSON over HTTPS (mTLS for provider authentication)

---

## Overview

The Energy Market RPC provides oracle integration, settlement tracking, and dispute resolution for on-chain energy trading. All endpoints enforce strict authentication, signature verification, and rate limiting.

### Authentication Model

**Roles**:
- **Provider**: Can submit meter readings, manage stake
- **Oracle**: Can attest to readings, dispute resolutions
- **Admin**: Emergency controls and parameter changes

**Key Requirements**:
- Ed25519 signature on all provider submissions
- mTLS certificate for transport security
- Clock skew tolerance: ±300 seconds
- Nonce-based replay protection

---

## Core Endpoints

### 1. Register Provider

**Endpoint**: `POST /energy/provider/register`  
**Auth**: Key pair (Ed25519)  
**Description**: Register as an energy provider  

**Request**:
```json
{
  "provider_id": "provider_usa_001",
  "owner": "ct1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe4tqx9",
  "location": "US-CA",
  "capacity_kwh": 1000000,
  "price_per_kwh": 5000,
  "meter_address": "meter_smart_001",
  "stake_amount_ct": 500000,
  "public_key": "base64(ed25519_pubkey)",
  "signature": "base64(ed25519_sig)"
}
```

**Response** (201 Created):
```json
{
  "provider_id": "provider_usa_001",
  "status": "active",
  "registered_at": 1702944000,
  "stake_amount_ct": 500000,
  "reputation_score": 0.5
}
```

**Errors**:
- `400 Bad Request`: Invalid provider data
- `409 Conflict`: Provider already registered
- `422 Unprocessable Entity`: Signature verification failed
- `402 Payment Required`: Insufficient stake

---

### 2. Submit Meter Reading

**Endpoint**: `POST /energy/reading/submit`  
**Auth**: Provider Ed25519 signature  
**Description**: Submit authenticated meter readings for settlement  

**Request**:
```json
{
  "provider_id": "provider_usa_001",
  "meter_address": "meter_smart_001",
  "total_kwh": 1500000,
  "timestamp": 1702944000,
  "signature": "base64(ed25519_sig(provider_id||meter||kwh||timestamp))",
  "nonce": 12345
}
```

**Signature Format**:
```
Ed25519_Sign(
  provider_id || 
  meter_address || 
  total_kwh (u64 LE) || 
  timestamp (u64 LE) || 
  nonce (u64 LE)
)
```

**Response** (202 Accepted):
```json
{
  "reading_hash": "0x3f4e...",
  "credit_amount_kwh": 50000,
  "status": "pending_settlement",
  "expires_at": 1702950000
}
```

**Validations**:
- ✓ Signature verification (Ed25519)
- ✓ Timestamp within ±300 seconds
- ✓ Total kWh monotonically increasing
- ✓ Nonce not previously used (replay protection)
- ✓ Provider active and solvent

**Errors**:
- `400 Bad Request`: Malformed request
- `422 Unprocessable Entity`: Signature failed, timestamp skew, meter anomaly
- `429 Too Many Requests`: Rate limit exceeded
- `402 Payment Required`: Provider balance insufficient

---

### 3. Get Market State

**Endpoint**: `GET /energy/market/state`  
**Auth**: None (public read)  
**Description**: Fetch current market state and provider inventory  

**Response** (200 OK):
```json
{
  "current_epoch": 1001,
  "total_capacity_kwh": 100000000,
  "available_capacity_kwh": 45000000,
  "active_providers": 247,
  "pending_credits_kwh": 5200000,
  "pending_disputes": 3,
  "providers": [
    {
      "provider_id": "provider_usa_001",
      "location": "US-CA",
      "capacity_kwh": 1000000,
      "available_kwh": 800000,
      "price_per_kwh": 5000,
      "reputation_score": 0.92,
      "reputation_confidence": 0.88,
      "staked_ct": 500000,
      "total_delivered_kwh": 5400000,
      "last_settlement": 1702944000,
      "slash_history": []
    },
    { ... }
  ]
}
```

---

### 4. Get Provider Details

**Endpoint**: `GET /energy/provider/:provider_id`  
**Auth**: None (public read)  
**Description**: Detailed provider information and reputation  

**Response** (200 OK):
```json
{
  "provider_id": "provider_usa_001",
  "owner": "ct1qqq...",
  "location": "US-CA",
  "capacity_kwh": 1000000,
  "available_kwh": 800000,
  "price_per_kwh": 5000,
  "staked_ct": 500000,
  "total_delivered_kwh": 5400000,
  "status": "active",
  "reputation": {
    "composite_score": 0.92,
    "confidence": 0.88,
    "delivery_alpha": 92.0,
    "delivery_beta": 8.0,
    "meter_alpha": 88.0,
    "meter_beta": 12.0,
    "latency_alpha": 85.0,
    "latency_beta": 15.0,
    "capacity_alpha": 90.0,
    "capacity_beta": 10.0,
    "total_observations": 500
  },
  "metrics": {
    "settlement_success_rate": 0.995,
    "avg_latency_ms": 1200,
    "uptime_pct": 0.9998,
    "avg_price_per_kwh": 5050
  },
  "slash_history": [
    {
      "reason": "meter_fraud",
      "amount_slashed_ct": 10000,
      "slashed_at": 1702900000,
      "disputed": false
    }
  ]
}
```

---

### 5. Get Pending Credits

**Endpoint**: `GET /energy/credits?provider_id=&status=pending`  
**Auth**: None (public read)  
**Description**: List energy credits awaiting settlement  

**Query Parameters**:
```
GET /energy/credits?provider_id=provider_usa_001&status=pending&limit=50
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `provider_id` | string | Filter by provider |
| `status` | string | pending, settled, expired, disputed |
| `min_kwh`, `max_kwh` | u64 | Credit size range |
| `limit`, `cursor` | u64 | Pagination |

**Response** (200 OK):
```json
{
  "credits": [
    {
      "reading_hash": "0x3f4e...",
      "provider_id": "provider_usa_001",
      "amount_kwh": 50000,
      "timestamp": 1702944000,
      "status": "pending_settlement",
      "expires_at": 1702950000,
      "settlement_amount_ct": 250000
    },
    { ... }
  ],
  "next_cursor": "0x5f7a...",
  "total_pending_kwh": 5200000,
  "total_pending_ct": 26000000
}
```

---

### 6. Settle Energy

**Endpoint**: `POST /energy/settle`  
**Auth**: Oracle role  
**Description**: Execute settlement and emit receipts  

**Request**:
```json
{
  "reading_hash": "0x3f4e...",
  "buyer": "ct1aaa...",
  "seller_provider_id": "provider_usa_001",
  "kwh_delivered": 50000,
  "price_per_kwh": 5000,
  "treasury_fee_bps": 250,
  "signature": "base64(oracle_sig)"
}
```

**Response** (201 Created):
```json
{
  "receipt": {
    "buyer": "ct1aaa...",
    "seller": "provider_usa_001",
    "kwh_delivered": 50000,
    "price_paid": 250000,
    "block_settled": 1001,
    "treasury_fee": 625,
    "slash_applied": 0,
    "tx_hash": "0x1234..."
  },
  "status": "finalized"
}
```

---

### 7. File Dispute

**Endpoint**: `POST /energy/dispute`  
**Auth**: Provider or Admin  
**Description**: Challenge a settlement or meter reading  

**Request**:
```json
{
  "reading_hash": "0x3f4e...",
  "provider_id": "provider_usa_001",
  "dispute_type": "meter_fraud | failed_delivery | price_dispute",
  "reason": "Meter reading inconsistency detected",
  "evidence_uri": "ipfs://QmXxxx",
  "signature": "base64(provider_sig)"
}
```

**Response** (202 Accepted):
```json
{
  "dispute_id": "dispute_001",
  "status": "pending_investigation",
  "created_at": 1702944000,
  "resolution_deadline_epoch": 1005
}
```

**Errors**:
- `409 Conflict`: Reading already settled/expired
- `410 Gone`: Dispute already filed

---

### 8. Get Disputes

**Endpoint**: `GET /energy/disputes?status=pending`  
**Auth**: None (public read)  
**Description**: List active and resolved disputes  

**Response** (200 OK):
```json
{
  "disputes": [
    {
      "dispute_id": "dispute_001",
      "reading_hash": "0x3f4e...",
      "provider_id": "provider_usa_001",
      "dispute_type": "meter_fraud",
      "reason": "Meter reading inconsistency",
      "filed_at": 1702944000,
      "status": "pending_investigation",
      "resolution_deadline_epoch": 1005,
      "outcome": null
    },
    { ... }
  ],
  "pending_count": 3,
  "total_slashed_ct": 45000
}
```

---

## Error Contract

### Error Response Format

```json
{
  "error": "signature_verification_failed",
  "message": "Ed25519 verification failed: invalid format",
  "code": 422,
  "context": {
    "provider_id": "provider_usa_001",
    "reason": "invalid_format"
  }
}
```

### Standard Error Codes

| Code | Type | Description |
|------|------|-------------|
| `400` | Bad Request | Malformed request |
| `402` | Payment Required | Insufficient stake or balance |
| `409` | Conflict | State transition not allowed |
| `410` | Gone | Resource expired or settled |
| `422` | Unprocessable Entity | Signature, timestamp, meter anomaly |
| `429` | Too Many Requests | Rate limit exceeded |
| `500` | Internal Server Error | Oracle processing error |

### Specific Error Reasons

```json
{
  "error": "signature_verification_failed",
  "reason": "invalid_format" // invalid_format, verification_failed, key_not_found, scheme_unsupported
}

{
  "error": "timestamp_skew",
  "detail": "Clock skew exceeds ±300 seconds"
}

{
  "error": "meter_anomaly",
  "reason": "total_kwh_decreased" // total_kwh_decreased, stale_reading
}

{
  "error": "provider_inactive",
  "reason": "poor_reputation" // poor_reputation, slashed, unregistered
}
```

---

## Rate Limiting

Rate limits vary by operation:

| Operation | Limit | Burst |
|-----------|-------|-------|
| Read (GET) | 1000/min | 200 |
| Submit Reading | 100/min | 20 |
| Settle | 50/min | 10 |
| File Dispute | 20/min | 5 |

**Headers**:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 92
X-RateLimit-Reset: 1702944060
X-RateLimit-RetryAfter: 5
```

---

## Authentication Examples

### Ed25519 Signature Generation

```python
import ed25519
import struct

def sign_meter_reading(private_key_b64, provider_id, meter, total_kwh, timestamp, nonce):
    # Load private key
    private_key_bytes = base64.b64decode(private_key_b64)
    signing_key = ed25519.SigningKey(private_key_bytes)
    
    # Build message
    message = (
        provider_id.encode() +
        meter.encode() +
        struct.pack('<Q', total_kwh) +
        struct.pack('<Q', timestamp) +
        struct.pack('<Q', nonce)
    )
    
    # Sign
    signature = signing_key.sign(message).signature
    return base64.b64encode(signature).decode()

# Usage
sig = sign_meter_reading(
    "base64_private_key",
    "provider_usa_001",
    "meter_smart_001",
    1500000,
    1702944000,
    12345
)
```

### mTLS Configuration

```bash
# Client certificate and key
export CLIENT_CERT="/path/to/client.crt"
export CLIENT_KEY="/path/to/client.key"

# Submit reading with mTLS
curl -X POST https://oracle.block.test/energy/reading/submit \
  --cert $CLIENT_CERT \
  --key $CLIENT_KEY \
  --cacert /path/to/ca.crt \
  -H "Content-Type: application/json" \
  -d @reading.json
```

---

## Monitoring and Debugging

### Key Metrics

- `energy_provider_total` — Active providers
- `energy_pending_credits_total` — Unsettled kWh
- `energy_settlements_total{provider}` — Settlement count
- `oracle_latency_seconds` — Verification latency
- `energy_signature_verification_failures_total` — Auth failures
- `energy_slashing_total{provider,reason}` — Slashing events

### Debugging Commands

```bash
# Check provider status
tb-cli energy provider show provider_usa_001

# List pending credits
tb-cli energy credits list --status pending --limit 20

# Check disputes
tb-cli energy disputes list --status pending

# View metrics
tb-cli metrics summary | grep energy_
```

---

## Governance Parameters

Energy market parameters controlled via governance:

```rust
pub struct EnergyMarketParams {
    pub min_stake: u64,              // Minimum provider stake
    pub treasury_fee_bps: u16,       // Fee basis points
    pub oracle_timeout_blocks: u64,  // Credit expiry
    pub slashing_rate_bps: u16,      // Penalty rate
    pub dynamic_fees_enabled: bool,
    pub bayesian_reputation_enabled: bool,
}
```

Update via governance proposal using `ParamSpec` payload.

---

## Production Checklist

Before accepting external provider registrations:

- [ ] All oracle endpoints passing signature verification tests
- [ ] Rate limiting active on all endpoints
- [ ] mTLS enforced for provider submissions
- [ ] Nonce replay protection enabled
- [ ] Timestamp skew bounds (±300s) enforced
- [ ] Bayesian reputation system tracking 500+ observations per provider
- [ ] Dispute resolution SLO < 3 epochs
- [ ] Slashing incidents logged and verified
- [ ] Settlement audit passing (ledger conservation)
- [ ] Monitoring dashboards green (latency, error rates, disputes)

---

For troubleshooting: See `docs/operations.md#energy-stalled`
