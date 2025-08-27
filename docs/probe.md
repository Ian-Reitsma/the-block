# Probe CLI

`probe` is a lightweight health-check binary used for synthetic monitoring of The-Block nodes.

## Usage

```
probe ping-rpc --timeout 2 --expect 500
probe mine-one --expect 1
probe gossip-check --addr 127.0.0.1:3030
probe tip --expect 10
```

Exit codes: `0` success, `2` soft failure (latency or expectation not met), `1` hard error.

Pass `--prom` to emit Prometheus metrics instead of plain text.

## Synthetic sweep

`scripts/synthetic.sh` executes `mine-one`, `gossip-check`, and `tip` sequentially and outputs `synthetic_*` metrics for Prometheus scraping.
