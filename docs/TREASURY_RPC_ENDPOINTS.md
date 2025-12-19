# Treasury RPC Endpoints

**Specification**: JSON-RPC 2.0 API for Treasury System Management  
**Protocol**: HTTP POST to `http://localhost:8000/rpc`  
**Authentication**: Bearer token in Authorization header (if enabled)  
**Content-Type**: `application/json`  

---

## 1. Query Treasury Balance

**Method**: `gov.treasury.balance`

**Description**: Get current treasury balance and executor status

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "gov.treasury.balance",
  "params": {
    "account_id": "treasury_main"
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "balance_ct": 10500000,
    "balance_it": 2100,
    "executor": {
      "last_error": null,
      "last_success_at": 1671447600,
      "pending_matured": 15,
      "lease_holder": "node_a",
      "lease_expires_at": 1671451200
    }
  }
}
```

**Error Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "Account not found",
    "data": {
      "account_id": "treasury_main"
    }
  }
}
```

---

## 2. List Disbursements

**Method**: `gov.treasury.list_disbursements`

**Description**: List disbursements filtered by status

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "gov.treasury.list_disbursements",
  "params": {
    "status": "queued",
    "limit": 50,
    "offset": 0
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "disbursements": [
      {
        "id": 1001,
        "status": "queued",
        "amount_ct": 50000,
        "recipient": "account_xyz",
        "created_at": 1671441000,
        "updated_at": 1671445800,
        "dependencies": [999, 1000],
        "memo": "{\"depends_on\": [999, 1000]}"
      },
      {
        "id": 1002,
        "status": "queued",
        "amount_ct": 75000,
        "recipient": "account_abc",
        "created_at": 1671442200,
        "updated_at": 1671445900,
        "dependencies": [],
        "memo": ""
      }
    ],
    "total_count": 142,
    "has_more": true
  }
}
```

**Status Values**: `draft`, `voting`, `queued`, `timelocked`, `executed`, `finalized`, `rolled_back`

---

## 3. Get Disbursement Details

**Method**: `gov.treasury.get_disbursement`

**Description**: Retrieve full details of a specific disbursement

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "gov.treasury.get_disbursement",
  "params": {
    "id": 1001
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "id": 1001,
    "status": "queued",
    "amount_ct": 50000,
    "recipient": "account_xyz",
    "created_at": 1671441000,
    "updated_at": 1671445800,
    "dependencies": [999, 1000],
    "memo": "{\"depends_on\": [999, 1000], \"reason\": \"Phase 2 funding\"}",
    "dependency_status": {
      "999": {"status": "executed", "verified": true},
      "1000": {"status": "executed", "verified": true}
    },
    "approvals": {
      "operator_1": {"timestamp": 1671441500, "sig": "0x..."},
      "builder_1": {"timestamp": 1671442000, "sig": "0x..."}
    },
    "executor_error": null
  }
}
```

---

## 4. Execute Disbursement

**Method**: `gov.treasury.execute_disbursement`

**Description**: Execute a matured disbursement (dependencies satisfied)

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "gov.treasury.execute_disbursement",
  "params": {
    "id": 1001,
    "tx_hash": "0xabcdef1234567890...",
    "receipts": [
      {
        "dependency_id": 999,
        "execution_hash": "0x...",
        "timestamp": 1671445000
      },
      {
        "dependency_id": 1000,
        "execution_hash": "0x...",
        "timestamp": 1671445100
      }
    ]
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "ok": true,
    "message": "Disbursement executed successfully",
    "execution_block": 12850,
    "transaction_hash": "0xabcdef1234567890..."
  }
}
```

**Error Responses**:
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "error": {
    "code": -32050,
    "message": "Dependencies not ready",
    "data": {
      "pending_dependencies": [999],
      "reason": "Dependency 999 still in 'timelocked' state"
    }
  }
}
```

---

## 5. Rollback Disbursement

**Method**: `gov.treasury.rollback_disbursement`

**Description**: Cancel a disbursement and restore funds

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "gov.treasury.rollback_disbursement",
  "params": {
    "id": 1001,
    "reason": "Recipient no longer eligible"
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "ok": true,
    "message": "Disbursement rolled back successfully",
    "refunded_amount_ct": 50000,
    "rollback_block": 12851
  }
}
```

---

## 6. Validate Dependency Graph

**Method**: `gov.treasury.validate_dependencies`

**Description**: Check for cycles and invalid dependencies

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "gov.treasury.validate_dependencies",
  "params": {
    "disbursement_id": 1001
  }
}
```

**Response** (Valid):
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "result": {
    "valid": true,
    "dependency_chain": [
      {"id": 999, "status": "executed"},
      {"id": 1000, "status": "executed"},
      {"id": 1001, "status": "ready"}
    ],
    "cycle_detected": false
  }
}
```

**Response** (Invalid - Cycle):
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "error": {
    "code": -32051,
    "message": "Circular dependency detected",
    "data": {
      "cycle": [1001, 1005, 1003, 1001],
      "reason": "Disbursement 1001 depends on 1005 which depends on 1003 which depends on 1001"
    }
  }
}
```

---

## 7. Get Executor Status

**Method**: `gov.treasury.executor_status`

**Description**: Get detailed executor health metrics

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "gov.treasury.executor_status",
  "params": {}
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "is_healthy": true,
    "lease_holder": "node_a",
    "lease_expires_at": 1671451200,
    "current_epoch": 425,
    "pending_matured": 15,
    "pending_immature": 42,
    "last_success_at": 1671447600,
    "last_error": null,
    "last_error_at": null,
    "consecutive_errors": 0,
    "uptime_seconds": 86400,
    "processed_count": 8542,
    "success_rate": 0.9998,
    "average_execution_time_ms": 125.5
  }
}
```

---

## Error Codes Reference

| Code | Meaning | Example |
|------|---------|----------|
| -32000 | Server error | Internal database failure |
| -32001 | Account not found | `treasury_main` doesn't exist |
| -32050 | Dependencies not satisfied | Dependency still in wrong state |
| -32051 | Circular dependency | Cycle detected in graph |
| -32052 | Insufficient funds | Can't execute (balance < amount) |
| -32053 | Invalid state transition | Already executed, can't rollback |
| -32100 | Parse error | Malformed JSON request |
| -32600 | Invalid request | Missing required parameter |
| -32601 | Method not found | `gov.invalid_method` |
| -32602 | Invalid params | `status: "invalid_status"` |
| -32603 | Internal error | Unexpected exception |

---

## Example: Complete Workflow

### Scenario: Execute Dependent Disbursements

**Step 1**: Check balance
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "gov.treasury.balance",
    "params": {"account_id": "treasury_main"}
  }'
```

**Step 2**: List queued disbursements
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "gov.treasury.list_disbursements",
    "params": {"status": "queued", "limit": 10}
  }'
```

**Step 3**: Check dependencies for disbursement #1001
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "gov.treasury.get_disbursement",
    "params": {"id": 1001}
  }'
```

**Step 4**: Validate dependency graph
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 4,
    "method": "gov.treasury.validate_dependencies",
    "params": {"disbursement_id": 1001}
  }'
```

**Step 5**: Execute (if all dependencies ready)
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 5,
    "method": "gov.treasury.execute_disbursement",
    "params": {
      "id": 1001,
      "tx_hash": "0x...",
      "receipts": [{"dependency_id": 999, "execution_hash": "0x...", "timestamp": 1671445000}]
    }
  }'
```

---

## Testing

**Quick health check**:
```bash
curl -s http://localhost:8000/rpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"gov.treasury.balance","params":{}}' | jq .
```

**Batch request** (multiple operations):
```bash
curl -X POST http://localhost:8000/rpc \
  -H "Content-Type: application/json" \
  -d '[{"jsonrpc":"2.0","id":1,"method":"gov.treasury.balance","params":{}},
       {"jsonrpc":"2.0","id":2,"method":"gov.treasury.list_disbursements","params":{"status":"queued"}}]'
```

---

**Specification Version**: 1.0  
**Last Updated**: 2025-12-19  
**Compatibility**: JSON-RPC 2.0 compliant  
