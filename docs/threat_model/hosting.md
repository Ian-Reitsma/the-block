# Hosting Threat Model

Edge hosting (gateways and wasm exec) attack vectors.

## Threats & Mitigations

- **Path-storm scraping** – gateways enforce per-IP token buckets.
- **Read-subsidy fraud** – batches require \>10 % client-signed `ReadAck` receipts; auditors sample and slash.
- **WASM abuse** – fuel metering and default gas limits abort runaway compute.
- **Domain squatting** – manifests require a stake deposit and minimum bytes.

Refer to [../gateway.md](../gateway.md) for service workflow details.
