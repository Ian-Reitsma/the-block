# Demo Guide

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
reflect the sliding‑window retarget algorithm. QUIC mode also reports that the
peer upgraded from TCP during the handshake.

For a step‑by‑step narrative of each phase in the script, including key
generation, transactions, and persistence, see [docs/explain.md](explain.md).
