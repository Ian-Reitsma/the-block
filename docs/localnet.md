# LocalNet Discovery and Sessions
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

LocalNet nodes discover nearby peers and exchange assist receipts without touching the wider network. Discovery runs over two short-range channels:

- **mDNS** – periodically advertises the node ID and supported features on the local subnet.
- **Bluetooth LE** – broadcasts the same payload for devices that are not on the same IP network.

Receivers validate advertisements and initiate an ECDH handshake. Each side sends its ephemeral public key and a signed nonce; the shared secret feeds a ChaCha20-Poly1305 session used for the receipt exchange. Peers disconnect immediately if the handshake fails or the signature is invalid.

Before accepting a receipt, the node checks a **proximity envelope** signed by the assisting device. The envelope encodes coarse GPS coordinates and a timestamp. Receipts outside the configured radius or stale beyond the tolerance window are rejected.

Proximity thresholds also depend on the submitting device class. `config/localnet_devices.toml` defines RSSI/RTT corridors for phones, laptops, and routers so operators can tune acceptance windows per hardware profile.

Devices that fail to meet the corridor requirements still complete the handshake
but their receipts are tagged `reason="distance"` and excluded from subsidy
claims. Operators can lower thresholds during disaster scenarios to widen
participation, but doing so may admit opportunistic long-range relays.

Once the session is established, clients submit assists through the `localnet.submit_receipt` RPC method:

```bash
curl -s 127.0.0.1:3030 -H 'Content-Type: application/json' -d \
'{"jsonrpc":"2.0","id":1,"method":"localnet.submit_receipt","params":{"receipt":"<hex>"}}'
```

The node verifies the receipt signature, enforces the proximity envelope, and records the receipt hash to prevent replays.

Telemetry surfaces `localnet_receipt_total` and `localnet_receipt_rejected_total{reason}` so operators can monitor LocalNet activity.

Example Grafana panels chart acceptance ratios by device class and map receipt
locations on an anonymized heat map, helping operators identify dead zones or
malicious nodes spamming out-of-range assists.

### CT-only genesis template

For devnets, bootstrap accounts using the provided CT-only genesis fixture at
`examples/genesis/genesis.json`.  The file allocates liquid CT and IT only.
Start a node with this genesis file to mirror main-net economic parameters.

## Mobile light-client hooks

The `light-client` crate exposes an FFI for verifying block headers and tracking
telemetry on resource-constrained devices. Mobile apps can link this library
and use LocalNet assists in the background; see `examples/mobile` for a file-based
header sync and basic wallet operations.