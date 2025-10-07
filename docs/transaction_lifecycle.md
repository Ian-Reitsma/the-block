# Transaction Lifecycle and Fee Lanes
> **Review (2025-09-25):** Synced Transaction Lifecycle and Fee Lanes guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The transaction subsystem coordinates account debits, signatures, and fee calculation while respecting dual fee lanes and the legacy industrial sub-ledger (retained for compatibility). This document walks through the full lifecycle from payload construction to on-chain inclusion and provides references for Python bindings and mempool behavior.

> **Bridge status:** The legacy PyO3 bindings have been retired. Until the
> first-party `python_bridge` crate exposes the new FFI behind the
> `python-bindings` feature, Python helpers such as `sign_tx_py` return
> feature-disabled errors so tooling can detect the gap early.

## 1. Raw Payload Structure

`node/src/transaction.rs` defines the `RawTxPayload` struct which is serialised with the canonical bincode configuration. Fields:

- `from_` / `to` – UTF-8 account identifiers. The Python bindings expose a `from` alias for ergonomics.
- `amount_consumer` – CT to transfer on the consumer lane.
- `amount_industrial` – CT routed through the industrial lane (legacy field; production policy keeps this zero).
- `fee` – absolute fee paid in CT regardless of lane.
- `pct_ct` – percentage of the fee paid in consumer tokens. Production policy pins this to `100`, but lower values remain available for simulations and regression tests.
- `nonce` – sequential per-sender nonce preventing replay; gaps are rejected by the mempool.
- `memo` – arbitrary byte vector stored verbatim; the mempool enforces a size cap to deter spam.

```rust
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct RawTxPayload {
    pub from_: String,
    pub to: String,
    pub amount_consumer: u64,
    pub amount_industrial: u64,
    pub fee: u64,
    pub pct_ct: u8,
    pub nonce: u64,
    pub memo: Vec<u8>,
}
```

## 2. Signing and Fee Lanes

Transactions are signed after serialising the payload. `SignedTransaction` bundles the payload, Ed25519 public key, signature, and a `FeeLane` enum:

```rust
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SignedTransaction {
    pub payload: RawTxPayload,
    pub public_key: Vec<u8>,
    pub signature: TxSignature,
    pub tip: u64,
    pub lane: FeeLane,
    pub version: TxVersion,
}
```

`FeeLane` has two variants:

- `Consumer` – default lane for retail traffic. Nodes prioritise consumer transactions when the 90th percentile fee exceeds a comfort threshold.
- `Industrial` – high-throughput lane for bulk workloads. Industrial transactions are deferred when consumer congestion rises.

Lane tags are part of the signed message. Malleating the tag invalidates the signature, making lane selection immutable post‑signing.

## 3. Python Bindings

PyO3 classes mirror Rust types enabling wallet tooling and tests:

```python
from the_block import RawTxPayload, SignedTransaction, FeeLane

payload = RawTxPayload(
    from_="alice", to="bob",
    amount_consumer=5, amount_industrial=0,
    fee=1, pct_ct=100, nonce=42, memo=b"hello"
)

# tip is optional and defaults to zero when omitted
signed = SignedTransaction(
    payload,
    pubkey_bytes,
    sig_bytes,
    FeeLane.Consumer,
    tip=2,
)
```

Helpers in `node/src/transaction.rs` expose `__repr__` and alias properties so Python users can inspect and mutate fields naturally.

> **PyO3 note:** Every static constructor exposed to Python must carry the
> `#[staticmethod]` attribute in Rust. Without it, PyO3 treats the first
> argument as `self` and runtime calls like `Blockchain.open(path)` will fail at
> import time.

## 4. Admission and Scheduling

1. **Submission** – Clients broadcast via the CLI (`blockctl tx send ...`), direct RPC, or Python bindings (`rpc.submit_tx`).
2. **Validation** – The node checks signature, fee sufficiency, nonce continuity, and memo length. Invalid transactions are rejected before hashing.
3. **Lane Mempools** – `node/src/mempool.rs` maintains separate heaps per lane sorted by effective fee. When capacity is exceeded low-fee entries are evicted.
4. **Comfort Guard** – A moving window monitors consumer p90 fees; if they exceed `comfort_threshold` industrial transactions pause until fees stabilise.
5. **Base Fee Accounting** – `node/src/fees.rs` updates the global base fee each block toward a fullness target. `pct_ct=100` transactions must include `fee >= base_fee` to remain valid.
6. **Parallel Execution** – On inclusion, the scheduler groups conflict‑free transactions so independent transfers across lanes execute concurrently; see `docs/scheduler.md`.

## 5. On‑Chain Effects and Receipts

Once executed, balance changes and memo bytes commit to the Merkle state root. Each block’s header records the lane-specific gas used and fee paid. Wallets track nonces to ensure subsequent transactions build on the correct state.

Anchored blocks expose their transactions through RPC (`ledger.block`). Indexers and explorers parse the payloads to display memos and lane information. Because lane tags are part of the signature, explorers can trust the fee classification without extra proofs.

## 6. Related References

- Mempool telemetry: `docs/metrics.md` (`mempool_size{lane}` and `tx_rejected_total{reason}`).
- Base fee and subsidy math: `docs/fees.md` and `docs/economics.md`.
- Python binding examples: `docs/contract_dev.md` and `README` sections on the Python module.

This document should serve as the authoritative transaction guide; update it whenever the wire format or fee logic changes.
