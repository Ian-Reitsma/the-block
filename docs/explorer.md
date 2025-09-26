# Explorer API
> **Review (2025-09-25):** Synced Explorer API guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The explorer exposes a lightweight REST service for querying on-chain data and analytics. DID anchors are cached in-process so repeated resolves hit memory rather than RocksDB, and the dedicated `did_view` page links anchors to wallet details for faster operator workflows.

## Endpoints

- `GET /blocks/:hash` – fetch a block by hash.
- `GET /blocks/:hash/proof` – fetch the light-client proof for a block.
- `GET /txs/:hash` – fetch a transaction by hash.
- `GET /gov/proposals/:id` – fetch a governance proposal.
- `GET /peers/reputation` – list peer reputation scores.
- `GET /peers/handshakes` – return handshake success and failure counts per peer.
- `GET /dex/order_book` – view the current DEX order book.
- `GET /dex/trust_lines` – list trust-line edges for visualization.
- `GET /compute/jobs` – list indexed compute jobs.
- `GET /dids` – list recent DID anchors or pass `address=<hex>` to fetch history for a specific account.
- `GET /identity/dids/:address` – return the latest DID document, hash, nonce, and optional remote attestation.
- `GET /dids/metrics/anchor_rate` – derive a per-second anchor rate series for dashboards.
- `GET /subsidy/history` – historic subsidy multipliers.
- `GET /mempool/fee_floor` – current dynamic fee floor, percentile, and window metadata.
- `GET /mempool/fee_floor_policy` – chronological fee-floor parameter updates captured from governance history.
- `GET /metrics/:name` – archived metric points for long‑term analysis.
- `GET /search/memo/:memo` – advanced search by memo.
- `GET /search/contract/:contract` – advanced search by contract hash.
- `GET /receipts/provider/:id` – receipts filtered by provider.
- `GET /receipts/domain/:id` – receipts filtered by domain.
- `GET /wasm/disasm?code=<hex>` – disassemble uploaded WASM bytecode.
- `GET /trace/:tx` – fetch opcode-level execution traces when available.

## Running with Docker Compose

`deploy/docker-compose.yml` includes an explorer service wired to a local node. Launch the stack with:

```bash
docker compose -f deploy/docker-compose.yml up
```

The explorer will listen on port `8080` and proxy requests to the first node in the stack.
