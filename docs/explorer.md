# Explorer

The explorer crate exposes a lightweight REST API for inspecting chain data and compute-market receipts.

## API Routes
- `GET /blocks/:hash` – fetch a block by hash
- `GET /txs/:hash` – fetch a signed transaction by hash
- `GET /gov/proposals/:id` – return a governance proposal payload
- `GET /peers/reputation` – list peer reputation scores
- `GET /search/memo/:memo` – find transactions containing the given memo substring
- `GET /search/contract/:contract` – find transactions targeting a contract
- `GET /receipts/provider/:id` – list compute receipts produced by a provider
- `GET /receipts/domain/:id` – list compute receipts purchased by a domain
- `GET /dex/order_book` – view aggregated DEX bids and asks
- `GET /compute/jobs` – list indexed compute jobs and their status

## Examples
```bash
# fetch a block
curl localhost:3001/blocks/abcd

# lookup transactions by memo
curl localhost:3001/search/memo/hello
```

## Docker
A simple Dockerfile is provided:
```bash
docker build -t explorer ./explorer
docker run -p 3001:3001 explorer /data/explorer.db /data/receipts
```
