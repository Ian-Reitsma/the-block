# Demo Walkthrough
> **Review (2025-09-25):** Synced Demo Walkthrough guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`demo.py` narrates the full journey of data through The‑Block.  Each
numbered section below maps to a portion of the script so readers can
relate printed output to blockchain concepts.

1. **Environment reset**
   - Seeds Python's RNG and removes any existing `chain_db` so every run
     starts from identical conditions.
   - The blockchain library's internal RNG uses the OS and therefore
     demonstrates true randomness when keys are generated.

2. **Chain instantiation**
   - Creates a new chain at difficulty `1` and immediately seals the
     genesis block, anchoring the ledger.
   - The chain length printed after this step shows the ledger holds a
     single block.

3. **Account creation**
   - Prepares four accounts: `miner`, `alice`, `bob`, and `faucet`, each
     starting with zero CT (the legacy industrial column is retained for compatibility and remains zero by policy).
   - Printing the balances highlights the dual‑token design that sets
     The‑Block apart from single‑asset chains.

4. **Keypair and signatures**
   - Generates an Ed25519 keypair and signs the message `b"hello"`.
   - The demo verifies the signature, illustrating how all
     transactions prove their origin.

5. **Fee matrix**
   - Calls `fee_decompose()` for selectors `0‒2` across several fee values, showing how each selector would split a payment; production pins selectors to `100`, but the demo illustrates the legacy behaviour used in regression tests.
   - Invalid selectors and overflow values trigger explicit errors,
     demonstrating safety checks in the API.

6. **Initial mining**
   - Mines the first block so the miner receives initial CT.
   - Validation and supply checks show the chain enforces token caps and
     consistent accounting.

7. **Transactions and errors**
   - Constructs a transaction from `miner` to `alice` and submits it to
     the mempool.
   - Replays the transaction to show duplicate rejection, then builds
     transactions with bad fee selectors and overflow fees to surface
     other failure modes.

8. **Mining loop**
   - Mines three additional blocks, printing each block's hash and the
     total circulating supply after the reward is issued.

9. **Emission cap**
   - Sets emission counters to one reward shy of their caps and mines a
     final block so the supply reaches, but does not exceed, the limit.
   - Calls `check_supply()` to assert the sum of all balances equals the
     circulating totals.

10. **Persistence**
    - Calls `persist_chain()` and reopens a chain to illustrate how the
      current prototype resets to genesis on restart.  Future versions
      will retain full history on disk.

11. **Cleanup**
    - Deletes `chain_db` so repeated runs begin fresh.

Running `python demo.py` prints detailed narration and exits with code
`0`.  Apart from the keypair, every run is deterministic.
