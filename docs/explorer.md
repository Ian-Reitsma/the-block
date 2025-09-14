# Explorer API

The explorer exposes a lightweight REST service for querying on-chain data and analytics.

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
- `GET /subsidy/history` – historic subsidy multipliers.
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
