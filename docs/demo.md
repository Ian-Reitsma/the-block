# Demo Guide
> **Review (2025-09-25):** Synced Demo Guide guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`demo.py` provides a lightweight walkthrough of The‑Block. It can mine blocks,
query RPCs, and optionally enable QUIC transport so users can observe
difficulty retargeting and network behaviour in real time.

## Usage

```bash
python demo.py           # deterministic run over TCP
python demo.py --quic    # spawn node with QUIC and poll difficulty
```

With `--quic`, the script launches a node listening on UDP, establishes a QUIC
connection, and prints the current difficulty every second:

```
starting difficulty: 1000
current difficulty: 1250
current difficulty: 1125
```

Difficulty values come from the `consensus.difficulty` JSON‑RPC endpoint and
reflect the multi‑window Kalman retune algorithm. QUIC mode also reports that the
peer upgraded from TCP during the handshake.

For a step‑by‑step narrative of each phase in the script, including key
generation, transactions, and persistence, see [docs/explain.md](explain.md).

> **Python tip:** When importing `the_block` from Python, call
> `Blockchain.open(path)` as a static constructor. The method is annotated with
> `@staticmethod` in PyO3 so the first argument is always treated as the path
> rather than an implicit instance.
>
> **Bridge status:** The legacy PyO3 bridge has been removed while the
> first-party `python_bridge` crate lands. Until the `python-bindings` feature
> exposes the new FFI, importing `the_block` will raise a feature-disabled
> error and the demo exits early.
