# World OS Energy Testnet Quickstart

This drill mirrors the production energy-market stack: the same governance parameters, signature verification logic, and dynamic fee formulas described in `docs/economics_and_governance.md` run here, just with mock oracle inputs and relaxed RPC auth. Launch Governor still manages readiness gates (operational + naming today, future energy gate under `AGENTS.md §15`), so treat this testnet as a staging ground for autopilot changes—shadow mode first, then apply once telemetry looks healthy. Watch the new `economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, `economics_epoch_treasury_inflow_block`, and `economics_block_reward_per_block` gauges; they are the same inputs the governor uses before flipping the testnet/mainnet economics gate.

Use this flow to exercise the end-to-end energy market: registering providers, submitting signed meter readings, validating credits, settling receipts, and rehearsing disputes before we open the lanes widely.

## Prerequisites
- Rust toolchain + workspace dependencies (`make bootstrap`). Run `cargo check -p energy-market -p oracle-adapter` once to pre-build the crates.
- Local telemetry stack (`docker/telemetry-stack.yml`) if you want Grafana/Prometheus dashboards during the drill.
- `.env` entries for RPC auth/rate limits (see `docs/operations.md`). `TB_RPC_AUTH_TOKEN` and `TB_RPC_ALLOWED_ORIGINS` protect `energy.*` endpoints the same way they protect the rest of RPC.
- Optional: set `TB_ENERGY_MARKET_DIR=/tmp/the-block-energy` if you want a disposable sled directory. Snapshots/restore tests work by swapping this directory, mirroring the production node restore path.

## 1. Build, Launch, and Tail Logs
```bash
./scripts/deploy-worldos-testnet.sh
journalctl -fu the-block-node.service | rg --line-buffered energy
```
The script compiles the node with the `worldos-testnet` feature, starts the bootstrap validator, launches the mock oracle on `127.0.0.1:8080`, and (when Docker is present) spins up Grafana/Prometheus. Tail the logs to watch meter submissions and settlements flow in real time.

## 2. Register as Provider
```bash
contract-cli energy register \
  10000 100 \
  --meter-address mock_meter_1 \
  --jurisdiction US_CA \
  --stake 5000 \
  --owner acct_energy_demo
```
`contract-cli energy register` talks to `http://localhost:26658` by default. The response immediately includes the assigned `provider_id`. Run the state query below to confirm capacity/price landed.

## 3. Query Market State and Export Receipts
```bash
contract-cli energy market --provider-id energy-0x00
contract-cli energy market --verbose > /tmp/energy_snapshot.json
```
The snapshot includes every provider, outstanding credit (`meter_hash`, `amount_kwh`, `timestamp`), and historical receipt. Use `--verbose` to capture the raw JSON; explorers and scripts can ingest it directly.

## 4. Submit Oracle Readings (Manual or Adapter)
```bash
reading='{"provider_id":"energy-0x00","meter_address":"mock_meter_1","kwh_reading":12000,"timestamp":1710000000,"signature":"deadbeef"}'
contract-cli energy submit-reading --reading-json "$reading"
```
- In production the `oracle-adapter` crate feeds these readings; for the testnet you can post JSON manually or point the adapter at the mock service (`http://127.0.0.1:8080/readings/<meter>`).
- `OracleAdapter` now enforces Ed25519 signatures whenever a provider registers a public key via `Ed25519SignatureVerifier`. To dry-run verification locally, add the provider to `config/default.toml` (`energy.provider_keys = [{ provider_id = "energy-0x00", public_key_hex = "<32-byte hex>" }, … ]`), reload the config to hot-swap the registry, recompute `MeterReading::signing_bytes`, sign it with your key material, and post the payload through RPC; unregistered providers continue to run in shadow mode for gradual rollout.
- The node enforces the same RPC auth/rate-limit policy (`TB_RPC_AUTH_TOKEN`, mutual TLS) that protects other namespaces. Expect structured error responses for bad signatures, stale timestamps, or unknown meter hashes; the CLI surfaces them via `--format json` so integration tests can match on the error code/message tuple described in `docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`.

## 5. Validate Meter Hashes and Credits
Every reading produces a BLAKE3 hash that shows up under the `credits` list. Verify it locally before settling:
```bash
python - <<'PY'
import blake3, json, sys
payload = json.loads(sys.argv[1])
h = blake3.blake3()
h.update(payload["provider_id"].encode())
h.update(payload["meter_address"].encode())
h.update(int(payload["kwh_reading"]).to_bytes(8, "little"))
h.update(int(payload["timestamp"]).to_bytes(8, "little"))
h.update(len(bytes.fromhex(payload["signature"])).to_bytes(4, "little"))
h.update(bytes.fromhex(payload["signature"]))
print("meter_hash=", h.hexdigest())
PY "$reading"
```
Compare the output to `contract-cli energy market --provider-id energy-0x00 --verbose | jq '.credits[0].meter_hash'`.

## 6. Settle Consumption and Inspect Receipts
```bash
contract-cli energy settle energy-0x00 500 --meter-hash e3c3... --buyer acct_demo_consumer
```
The RPC verifies the credit, applies treasury/slashing math, and emits an `EnergyReceipt` anchored inside the BLOCK ledger snapshot. Re-run `contract-cli energy market` to confirm the credit decreased and the receipt shows up in the `receipts` array. Capture receipts for explorers with:
```bash
jq '.receipts' /tmp/energy_snapshot.json > explorer/fixtures/energy_receipts.json
```

## 7. Dispute, Rollback, and Param Retune Drills
Use the dedicated dispute RPCs + CLI to rehearse investigations before falling back to governance knobs:
1. Capture the suspect `meter_hash`/`provider_id` from `contract-cli energy market --verbose` or `contract-cli energy credits --provider-id energy-0x00`.
2. File a dispute:
   ```bash
   contract-cli energy flag-dispute \
     --meter-hash e3c3... \
     --reason "Provider energy-0x00 reported 500kWh while the meter was offline" \
     --reporter ops-team
   ```
   The RPC records the reporter, reason, meter hash, provider, and timestamp.
3. List open disputes (`contract-cli energy disputes --status open`) or paginate them for explorer ingestion with `--json --page/--page-size`.
4. Audit historical settlements with `contract-cli energy receipts --provider-id energy-0x00 --json > receipts.json`.
5. Resolve disputes once remediated:
   ```bash
   contract-cli energy resolve-dispute \
     --dispute-id 1 \
     --resolver ops-team \
     --resolution-note "Meter replaced; credit invalidated and buyer refunded"
   ```
   The RPC stamps the resolution timestamp/resolver/note so dashboards stay in sync.
6. If a dispute uncovers systemic issues, you can still tighten governance parameters (e.g. `energy_slashing_rate_bps`) via `contract-cli gov param update`, but run the dispute drill first so the sled log, CLI, and dashboards all reflect the investigation history.

## 8. Dashboards, Monitoring, and SLOs
Use `contract-cli energy slashes` or the `/governance/energy/slashes` explorer route to inspect anything the metrics stream pinpoints. Watch the new panels for `energy_quorum_shortfall_total`, `energy_reading_reject_total`, and `energy_dispute_total` alongside the existing slash/settlement charts. You can also run Prometheus queries directly:

```bash
contract-cli energy slashes --provider-id energy-0x00 --json
prometheus_query 'rate(energy_quorum_shortfall_total[5m])'
prometheus_query 'rate(energy_reading_reject_total[5m])'
prometheus_query 'energy_dispute_total'
```

- Grafana: `http://localhost:3000` (default `admin/admin`). Panels show `energy_providers_count`, `energy_kwh_traded_total`, `energy_avg_price`, slash totals, and oracle latency histograms exported by `crates/energy-market`. Add alerts that trigger whenever pending credits exceed 25 or when oracle latency > threshold.
- Metrics aggregator: `make monitor` exposes `/telemetry/summary` and `/wrappers` so you can scrape energy KPIs alongside transport/runtime metadata. The `energy_providers_count` and `oracle_reading_latency_seconds` series feed the default dashboards.
- Health checks: `node::energy::check_energy_market_health` logs warnings if oracle latency or pending credits trip the thresholds. Monitor the log stream or scrape `journalctl` for `energy market pending credits`.
- Governance review: `contract-cli gov energy-settlement --timeline` prints the persisted mode/timeline log, `--dry-run` renders the payload, and the explorer endpoint `/governance/energy/settlement/history` exposes the same records for auditors/telemetry.

## 9. Snapshot, Restore, and Chaos Drills
- **SimpleDb + sled snapshots** — Stop the node, copy `$TB_ENERGY_MARKET_DIR`, restart on staging, and run `contract-cli energy market --verbose` to confirm byte-identical state. Log the drill duration, `energy_snapshot_duration_seconds`, and any errors surfaced via `/wrappers`. Repeat after schema changes.
- **QUIC + transport chaos** — While the testnet node is live, run the WAN-scale drill (`scripts/chaos_quic.sh`) to fault oracle transport providers, rotate mutual-TLS fingerprints, and confirm `quic_failover_total`/`transport_capability_mismatch_total` stay within expectations. Capture Grafana screenshots and attach them to the drill log so operators can rehearse failure recovery before production changes.

## 9. Feedback Loop
Open GitHub Discussions tagged `worldos/energy` or the `#world-os-energy` Discord channel to report issues. Include:
- The JSON returned by `contract-cli energy market --verbose`.
- IDs from `contract-cli energy disputes --status open` (and any resolution notes you recorded).
- Relevant Grafana screenshots + `/telemetry/summary` output.
This lets us reproduce signature/latency bugs quickly and keeps the docs aligned with the latest node behavior.
