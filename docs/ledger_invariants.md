# Ledger Invariants

This document enumerates the key invariants that underpin balance and nonce
correctness.  Each is referenced by a stable ID so logs and metrics can refer to
them unambiguously.

This chapter details how The‑Block enforces a one‑to‑one mapping between submitted transactions and account state.

## Transaction Flow

1. **Submission** – A signed transaction enters the node via `submit_transaction`.
   Stateless checks validate the signature and structure.
2. **Balance Check** – The node computes effective balances:

   ```text
   effective_consumer   = balance.consumer   − pending_consumer
   effective_industrial = balance.industrial − pending_industrial
   ```

   The transaction is rejected if the debit plus fee exceeds either effective balance.
3. **Reservation** – On success the account reserves the amounts and fee by
   increasing `pending_consumer`, `pending_industrial`, and `pending_nonce`.
   No further transaction from the same sender is allowed unless the nonce is
   exactly `account.nonce + pending_nonce + 1`.
4. **Mining** – When a block includes the transaction the reserved values are
   deducted from `balance.*` and the pending fields decrement to reflect the
   remaining in‑flight transactions.
5. **Eviction/Reorg** – If a transaction is dropped from the mempool or a block
   containing it is reorged out, the reservation rolls back atomically so funds
   become available again.

## Invariants

* **INV-PENDING-001** – At any time there is at most one committed or pending
  transaction with nonce `N = account.nonce + pending_nonce + 1`.
* **INV-PENDING-002** – `balance.consumer + pending_consumer` and
  `balance.industrial + pending_industrial` are never negative.
* **INV-PENDING-003** – `pending_nonce` equals the number of transactions for the
  account currently in the mempool.
* **INV-PENDING-004** – Atomicity of reservations:

  \[
  \forall t.\; B_i^{\text{confirmed}}(t) + P_i(t)
  = B_i^{\text{confirmed}}(t_0) - \sum_k d_i(k)
  \]

  where `i` ranges over consumer and industrial token classes and `d_i(k)` is the
  debit in transaction `k`.

## Nonce and Supply Invariants

* **INV-NONCE-001** – Account nonces increase by exactly one for each mined
  transaction. Property test `nonce_supply_prop` submits randomized sequences and
  asserts the sender's nonce matches `previous_nonce + 1` after every block.
* **INV-SUPPLY-001** – The sum of all account balances equals the total emitted
  supply after each block. The same property test confirms `Σ balances == Σ
  emitted` regardless of transaction order.

## State Diagrams

Below is a simplified example of a single account sending one transaction.

### Before Submission

```
nonce           = 0
pending_nonce   = 0
balance.consumer= 10
pending_consumer= 0
```

### After `tx1` (amount=2, fee=1)

```
nonce           = 0
pending_nonce   = 1
balance.consumer= 10
pending_consumer= 3
```

### After Block Mined

```
nonce           = 1
pending_nonce   = 0
balance.consumer= 7   # 10 − 3
pending_consumer= 0
```

Reorg or explicit eviction reverses the reservation step so the "Before" state
is restored.

### Mixed Consumer/Industrial Example

```text
nonce                  = 0
pending_nonce          = 0
balance.consumer       = 20
balance.industrial     = 15
pending_consumer       = 0
pending_industrial     = 0
```

After a transaction debiting 5 consumer and 4 industrial with fee 1:

```text
nonce                  = 0
pending_nonce          = 1
balance.consumer       = 20
balance.industrial     = 15
pending_consumer       = 6  # 5 + fee
pending_industrial     = 4
```

Once mined the reservations convert to real debits and pending fields drop to
zero.

## Fork and Reorg Example

| Step | nonce | pending_nonce | consumer | pending_consumer |
|-----|------|---------------|---------|------------------|
| Before fork | 3 | 0 | 5 | 0 |
| Submit tx4  | 3 | 1 | 5 | 1 |
| Fork mined  | 4 | 0 | 4 | 0 |
| Reorg back  | 3 | 1 | 5 | 1 |

During a reorg, the block containing `tx4` is removed and the reservation returns, restoring balances as if the transaction were pending again.
