# Hosting Threat Model
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

## Per-IP Throttling
Gateways enforce a token-bucket limit for every client IP. The default budget allows roughly twenty requests per second and slowly refills to accommodate bursty traffic. When a client exceeds the bucket, the gateway replies with HTTP 429 and increments the `read_denied_total{reason="rate_limit"}` counter. Operators can tune the limit through governance or environment overrides, but a limit must always exist to bound worst-case amplification.

Attack scenario: a botnet rotates through a dictionary of URLs attempting to
inflate a target site's read subsidy. The token bucket caps each source IP's
throughput; sustained scraping requires thousands of source addresses, driving
the attack cost up. Governance can temporarily lower the global `gateway.req_rate_per_ip`
parameter if a widespread crawl is detected, and operators should monitor the
`read_denied_total{reason="rate_limit"}` metric for sudden spikes.

## ReadAck Requirements
Reads only earn subsidies after the requesting client signs a compact `ReadAck` that binds the manifest identifier, byte count, and a recent epoch. Gateways batch these acknowledgements and claim `READ_SUB_CT` in the next block. Missing or forged acknowledgements halt subsidy accrual for that batch and can trigger stake slashing if a gateway repeatedly submits unverifiable claims.

`ReadAck` payloads include a salted hash of the client IP so auditors can
reconstruct unique visitor counts without storing raw addresses. Batches must
contain at least 10 % verifiable acknowledgements; otherwise the entire batch is
discarded and the gateway's bond is subject to slashing. Auditors randomly
sample batches and request proofs from gateways to maintain honest reporting.

## Wasm Fuel Limits
Dynamic sites execute in a deterministic WebAssembly sandbox. Each invocation receives a pre-allocated fuel budget derived from `func.gas_limit_default`. Exhausting fuel aborts the request, ensuring CPU usage stays within predictable bounds and preventing infinite loops or algorithmic complexity attacks.

Operators should size `func.gas_limit_default` based on the heaviest expected
endpoint and track `exec_abort_total{reason="fuel"}` telemetry. A sudden rise in
aborts usually indicates buggy or malicious code and should trigger a review of
the deployed WASM binaries.

## Domain Stake Deposits
Publishing a domain requires bonding CT in an escrow that is locked until a cool-down period elapses. This deposit deters domain squatting and enables punitive slashing for hosts that serve malicious content, ignore audit subpoenas, or attempt to Sybil the gateway role. Governance may raise the minimum bond if abuse patterns emerge.

The bond amount is defined by `caps.domain_stake_min_ct` and currently defaults
to 0.1 CT. Domains that fail to serve their manifest within the probation window
are automatically flagged and may have their bond slashed. Operators should use
`blockctl rpc stake.role gateway <account>` to verify that the bond is active
before advertising a new site.

## Exec Receipt Verification
Every dynamic request emits an `ExecReceipt` containing hashes of the request body, response body, byte counts, and the measured CPU milliseconds. Receipts are signed by the gateway and optionally co-signed by auditor nodes that re-execute the function with the same seed. Any mismatch between receipt claims and auditor replays voids the reward and slashes the gateway's bond.

Auditors store replay logs so disputed executions can be reproduced months
later. A gateway that consistently fails verification is quarantined from the
reward schedule until it passes a probation period with clean receipts.

### Cross-Cutting
- Bonded service roles tie eligibility to staked CT, making large-scale Sybil attacks economically expensive.
- Inflation governors automatically retune subsidy multipliers each epoch while the kill-switch parameter can globally downscale rewards during emergencies.
- Salted IP hashing obfuscates client addresses, preventing long-term correlation while still allowing per-epoch abuse tracking.
- Telemetry surfaces `subsidy_bytes_total{type="read"}` and `subsidy_cpu_ms_total` so operators and auditors can detect anomalies or capacity shortfalls in real time.
- Operators should alert on sustained deviations between claimed subsidies and
observed traffic patterns; a mismatch often indicates a compromised gateway or
misconfigured auditor network.

## SiteManifestTx & FuncTx Integrity
Publishing a site or dynamic endpoint requires a signed on-chain commitment.
`SiteManifestTx` binds a domain to the BLAKE3 root of its manifest, while
`FuncTx` commits to the WASM bytecode used for `"/api/*"` calls. Gateways verify
these commitments before serving content. Any mismatch between the served blob
and the committed root is slashable. Clients can independently hash responses to
ensure the gateway streamed bytes corresponding to the manifest.

## Analytics & Privacy
The `analytics` RPC aggregates `ReadAck` batches allowing publishers to audit
pageviews. Client IPs are salted per epoch before hashing, preventing long-term
tracking while still enabling unique visitor counts. Gateways that fabricate
analytics by omitting salted hashes risk subsidy claw-backs once auditors sample
their batches.