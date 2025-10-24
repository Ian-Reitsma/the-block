# Explorer API
> **Review (2025-09-25):** Synced Explorer API guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The explorer exposes a lightweight REST service for querying on-chain data and analytics. DID anchors are cached in-process so repeated resolves hit memory rather than RocksDB, and the dedicated `did_view` page links anchors to wallet details for faster operator workflows.

### Block payout breakdowns

Blocks indexed after the governance split now store per-role CT distributions for both read subsidies and advertising settlements. The `/blocks/:hash/payouts` endpoint surfaces those totals without requiring callers to decode the full binary block image; the explorer falls back to the JSON payload written to SQLite when the binary codec is unavailable (e.g., the stubbed test harness). Responses include the block hash, height, and two role maps (`read_subsidy`, `advertising`) with totals for viewers, hosts, hardware vendors, verifiers, the liquidity pool, and the residual miner share. The CLI mirrors this endpoint via `contract-cli explorer block-payouts`, accepting either a block hash or height and printing the JSON response directly for automation.

Integration coverage now pairs the JSON snapshots with binary block headers so decoder fallbacks stay verified when explorers mix historic and modern payloads in the same sync window. Unit coverage still exercises the JSON fallback with legacy snapshots that omit the per-role fields entirely, guaranteeing FIRST_PARTY_ONLY builds continue to render historical payouts even as the header shape evolves. The CLI command also validates that exactly one of `--hash` or `--height` is supplied and reports a clear error when a block is missing, keeping automation flows hermetic without shell scripting or third-party JSON tooling.

#### Automation examples

Hash-driven payout lookup via the CLI:

```bash
contract-cli explorer block-payouts --hash 0xabc123...
```

Height-driven payout lookup (the command resolves the hash automatically):

```bash
contract-cli explorer block-payouts --height 123456
```

Both commands emit the raw JSON payload so operators can feed the output directly into jq-alternatives such as the first-party `foundation_serialization` tooling, or redirect to files for downstream reconciliation jobs. To query the REST surface instead, resolve the hash (via the CLI `--height` helper or `GET /blocks/:hash`) and fetch the payout snapshot directly:

```bash
curl -sS "http://explorer.local:8080/blocks/0xabc123.../payouts" | foundation-json pretty
```

## Endpoints

- `GET /blocks/:hash` – fetch a block by hash.
- `GET /blocks/:hash/payouts` – return per-role CT totals for read subsidies and advertising flows persisted in the block header.
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
