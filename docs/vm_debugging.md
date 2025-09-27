# VM Debugging
> **Review (2025-09-25):** Synced VM Debugging guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The VM includes a lightweight step-by-step debugger when launched with `--enable-vm-debug`.

## CLI

```bash
contract debug <hex bytecode>
```

Commands:
- `s` – execute a single opcode
- `c` – continue until halt
- `q` – exit

Traces are written under `trace/last.json` and `trace/last.chrome.json` for offline analysis.

## RPC

Nodes started with `--enable-vm-debug` expose a websocket endpoint:

```
GET /vm/trace?code=<hex>
```

Each message contains the program counter, opcode, stack, and storage snapshot.

A Prometheus counter `vm_trace_total` tracks usage of this endpoint.
