# Security and Privacy

Security is enforced in code, not promises. This guide consolidates the former threat-model, bridge-security, privacy, and supply-chain docs.

## Who Are the "Bad Guys"?

> **Plain English:** Before diving into technical details, here's who we're protecting against:
>
> | Threat | What They Might Try | How We Defend |
> |--------|---------------------|---------------|
> | **Token thieves** | Steal BLOCK by forging signatures or exploiting bugs (code still shows BLOCK field names) | Ed25519/Dilithium signatures, `#![forbid(unsafe_code)]`, audit gates |
> | **Censors** | Block certain users or transactions | Decentralized consensus, multiple relayers, gossip protocols |
> | **Fake readings** | Submit false energy/compute data to earn undeserved rewards | Oracle signature verification, slashing for bad behavior |
> | **Data snoops** | Spy on what users are reading/storing | Encrypted payloads, privacy-preserving read receipts, no logging of content |
> | **Compromised nodes** | Rogue operators trying to manipulate state | Macro-block checkpoints, settlement audits, reputation slashing |
>
> The rest of this doc explains the technical mechanisms.

## Threat Model
- Adversaries include malicious operators, compromised gateways, colluding relayers, and jurisdiction-specific takedown requests.
- Consensus hardens liveness with hybrid PoW/PoS plus macro-block checkpoints; even if gossip partitions, PoH + VDF tie the timeline together.
- Gossip/range-boost nets track `partition_watch` metrics so partitions trigger incident playbooks before operators lose quorum.
- Storage/compute markets slash via `compute_market::settlement::SlaOutcome` and provider loss metrics in `node/src/storage/pipeline.rs`.

## Cryptography Stack
- `crypto_suite` and `crates/crypto` expose BLAKE3, Ed25519, Dilithium, Kyber, etc. All consensus + wallet code compiles with `#![forbid(unsafe_code)]`.
- Commit–reveal and DKG flows rely on PQ-ready fallbacks. `node/src/commit_reveal.rs` switches between Dilithium and BLAKE3; `dkg/` handles committee keys; `zkp/` contains SNARK verification code.
- Mathematical proofs remain under `docs/maths/` (LaTeX + PDF) and are referenced from CI + auditors.

## Remote Signers and Key Management

> **Plain English:** A remote signer is a separate, hardened machine that holds your private keys. Instead of keeping keys on the same computer that's connected to the internet, you:
> 1. Create an unsigned transaction on your regular machine
> 2. Send it to the air-gapped signer
> 3. The signer signs it and returns the signed transaction
> 4. Your regular machine broadcasts the signed transaction
>
> This way, even if your regular machine is compromised, your keys stay safe.

- Remote signer workflows live in `node/src/remote_signer_security.rs`, `cli/src/wallet.rs`, and `wallet/` crates. CLI enforces multisig, escrow-hash selection, and remote telemetry.
- Release provenance (`node/src/provenance.rs`) verifies binary hashes against signed allow lists; attested binaries roll back automatically if hashes drift.
- Environment variables `TB_RELEASE_SIGNERS`, `TB_RELEASE_SIGNERS_FILE` override defaults for air-gapped deployments.
- **Upcoming hardening (AGENTS.md §15.D)** — Wallet UX requires batched signer discovery, localized fee-floor warnings, JSON automation hooks, and telemetry surfaced through `metrics-aggregator` `/wrappers`. Every remote-signer change must update `docs/apis_and_tooling.md`, `docs/operations.md`, and law-enforcement portal metrics while adding regression tests under `tests/remote_signer_*.rs`.

## Energy Oracle Safety
- **Key sourcing** — Oracle adapters (`crates/oracle-adapter`) must draw signing keys from hardened storage (`TB_ORACLE_KEY_HEX`, hardware modules, or governance-approved secret stores). Never embed keys in code or logs. The in-tree Ed25519 verifier (`Ed25519SignatureVerifier`) now enforces signatures for every provider with a registered public key, so every meter reading must carry a valid signature over `MeterReadingPayload::signing_bytes()` before the adapter forwards it to the node. Operators define trusted provider keys in `config/default.toml` (`energy.provider_keys` array); reloading the config hot-swaps the verifier registry without restarts.
- **Transport & auth** — Oracle adapters send readings through the same HTTP/TLS stack as all other tooling (first-party `httpd::Client`). Configure mutual-TLS or RPC auth tokens (`TB_RPC_AUTH_TOKEN`) before enabling public ingestion. Rate limiting (`node/src/rpc/mod.rs::check_rate_limit`) applies to `energy.*` endpoints, so adapters should honour `429` responses and retry with jitter.
- **Telemetry redaction** — Meter readings flow through `node/src/rpc/energy.rs`. Logs must omit raw signatures and meter values unless `RUST_LOG=trace` is explicitly set. Oracle adapters should scrub meter IDs and signatures before logging; dashboards rely on aggregate metrics (`energy_kwh_traded_total`, `oracle_reading_latency_seconds`) instead of raw payloads.
- **Dispute hooks** — Until dedicated dispute RPCs ship, governance proposals (e.g., raising `energy_slashing_rate_bps`, pausing settlement) are the primary kill switch. Record suspect `meter_hash` values via `contract-cli energy market --verbose`, attach them to proposals, and document rollback steps. Once the dispute RPC/CLI pair lands they will emit ledger anchors referencing the disputed readings plus telemetry counters for slash totals.
- **Mock oracle isolation** — `services/mock-energy-oracle` is a dev/testnet binary only. It uses mock signatures (provider_id||kwh) and intentionally relaxed auth. Never expose it to production networks; wrap it in loopback-only listeners when exercising `scripts/deploy-worldos-testnet.sh`.
- **Release & supply chain** — Energy/oracle crates fall under the same release-provenance gates as the rest of the workspace: `cargo vendor` snapshots, `provenance.json` hashes, signed tags, and dependency audits must pass before shipping binaries that include `crates/energy-market` or `crates/oracle-adapter`. Secrets must be injected at runtime (env or KMS), not bundled into release artifacts.

## Privacy Layers
- Reads stay free by logging signed `ReadAck` receipts, not payloads. Operators can redact metadata via the privacy crate (`privacy/`) when the `privacy` feature is enabled.
- Read-ack privacy modes (`node/src/config.rs::ReadAckPrivacyMode` + `node/src/blockchain/privacy.rs`):
  - `Enforce` (default) — every receipt must include a privacy proof; failures raise `ReadAckError::PrivacyProofRejected`.
  - `Observe` — proofs are checked but failures are logged (`read_ack_privacy_verification_failed`) instead of rejected so operators can collect samples without losing revenue.
  - `Disabled` — privacy proof checks are skipped (only use during incident response). RPC `node.get_ack_privacy`/`node.set_ack_privacy` change modes live; CLI wrappers should restore `enforce` after drills.
- Law-enforcement portal (`node/src/le_portal.rs`) writes hashed case IDs and action logs; optional ChaCha20-Poly1305 evidence buckets live under `<base>/evidence/`.
- Range-boost mesh encrypts payloads, tracks hop proofs, and never exposes raw content to intermediate peers.
- **Ad market privacy budgets** — `crates/ad_market/src/privacy.rs` guards selector-level anonymity before readiness or inventory data leaves the node:
  - Governance configures `PrivacyBudgetManager::Config` (k-anonymity threshold, max selector dimensions, premium-domain guard toggles, presence precision caps, and the per-epoch `budget_ppm`). These knobs ride through `node/src/config.rs` and governance proposals handled by `cli/src/gov.rs`.
  - Selector combinations (domain tier/owner, provider, badges, interest tags, presence bucket) are hashed into a `SelectorFingerprint`. Each exposure charges against `budget_ppm`; when it hits zero, RPCs return `-32037` and dashboards emit `ad_privacy_budget_utilization_ratio` + `ad_privacy_denial_total{reason="budget_exhausted"}`.
  - No readiness payload leaves the node unless `ready_slots >= k_anonymity_threshold` and the presence radius is ≥ `presence_precision_cap_meters`. Otherwise responses fold into aggregate domain-tier stats and log `ad_privacy_denial_total{reason="k_anonymity"}` so operators know why the data was redacted.
  - Premium domains combined with sub-`presence_precision_cap_meters` buckets require explicit opt-in stored in campaign metadata (`presence_opt_in=true`). Violations raise `-32035` and surface in the readiness dashboard so advertisers see why cohorts were rejected.
  - Deterministic tests next to the manager (`privacy.rs::tests::privacy_budget_respects_k_anonymity`, `privacy_budget_rejects_forbidden_selector`) must be updated whenever governance changes any bound. Mirror the same cases in `node/tests/ad_market_rpc.rs` so RPC regressions cannot bypass the privacy contract.
  - CLI/explorer tooling must print the active privacy budget and `/wrappers` hash (`contract-cli ad-market readiness --with-privacy`) so auditors can compare what the RPC returned to what governance approved. Law-enforcement portal addenda record any override granted under subpoena along with the governance approval ID.

## KYC, Jurisdiction, and Compliance
- KYC provider flows live in `node/src/kyc.rs` plus the `jurisdiction/` crate. Policy packs encode consent defaults, languages, and feature toggles per region.
- Pack schema (`crates/jurisdiction/src/lib.rs`):

  ```json
  {
    "region": "US",
    "consent_required": true,
    "features": ["wallet","dex"],
    "parent": "NA"
  }
  ```

  Packs may inherit from a `parent` region; `PolicyPack::resolve()` flattens the tree so downstream services operate on the effective settings.
- Signed packs embed the pack JSON plus a 64-byte Ed25519 signature. `SignedPack::verify(vk)` enforces authenticity; feeds fetched via `fetch_signed(url, pk)` honour TLS settings (`TB_JURISDICTION_TLS`, `TB_HTTP_TLS`). CLI `jurisdiction.set` swaps packs, while `jurisdiction.policy_diff` compares two packs and highlights consent/feature changes.
- Governance proposals log pack hashes so explorers and dashboards can prove which policy applied at any height. Forked jurisdictions publish separate feeds and set `jurisdiction_region` accordingly.
- `docs/jurisdiction_authoring.md` content is folded here: versioned packs, governance-voted updates, optional forks for conflicting jurisdictions.
- Non-custodial core: ramps handle KYC/AML; the node never holds user secrets.

## Law-Enforcement Portal and Warrant Canary
- API surface (`node/src/le_portal.rs`):
  - `LeRequest { timestamp, agency, case_hash, jurisdiction, language }` and `LeAction { action_hash, … }` are serialized to JSON and appended to `<base>/le_requests.log` and `<base>/le_actions.log`. CLI commands (`contract-cli le request|action`) accept `--base <dir>`; default base is `./le_portal`.
  - Evidence uploads write raw bytes to `<base>/evidence/<hash>` and log `EvidenceRecord` JSON lines in `le_evidence.log`. Payloads are hashed via BLAKE3 before persistence for tamper detection.
  - Warrant canary entries append `<timestamp> <hash>` to `warrant_canary.log`. Operators publish signed statements out-of-band; if authorities compel silence the canary stops updating.
- Sanitisation hooks: when the optional `privacy` feature is enabled, `sanitize_payload` rejects memos outside the “local” jurisdiction before writing logs, and the audit sled mirrors every entry for later review.

## Risk Register and Incident Logging
- Former `docs/risk_register.md` entries are now structured as:
  - **Consensus** – watch for leader splits, PoH stalls, DKG transcript leaks.
  - **Networking** – QUIC/TLS misconfigs, peer DB corruption, overlay exhaustion.
  - **Storage/Compute** – erasure thresholds, SLA slashing, escrow exhaustion.
  - **Governance** – treasury drains, kill-switch toggles, badge forgeries.
  Log incidents via the metrics aggregator `/audit` endpoint and cross-link to this section.

## Bridge and Cross-Chain Security
- `bridges/` telemetry counters (`bridge_*`) highlight proof verification failures, disputes, liquidity changes, and slash events. Aggregator dashboards keep per-asset panels.
- Reward approval workflows require multisig attestations; CLI + explorer use the same code paths to prevent phantom unlocks.
- HTLC proofs and trust-line routing reuse the same ledger invariants so locked liquidity can’t leak.
- Upcoming work (`AGENTS.md §15.E`) adds signer-set payload documentation, telemetry for partial-payment retries, and release-verifier scripts. Every bridge/DEX PR must update these sections plus `docs/architecture.md#token-bridges` / `#dex-and-trust-lines` before merging.

## Release Provenance and Supply Chain
- Release provenance is enforced by `node/src/provenance.rs`, `config/release_signers.txt`, and the CI job that verifies `provenance.json` + `checksums.txt`.
- Dependency independence: first-party wrappers (`foundation_*` crates) replace third-party TLS/HTTP/serialization stacks. `docs/developer_handbook.md#dependency-policy` covers required tooling and audits.
- Reproducible builds: `docs/repro.md` + `docs/reproducible_builds.md` were merged here. Build IDs must match `env!("BUILD_BIN_HASH")` or binaries are rejected on startup.
- Energy/oracle crates (`crates/energy-market`, `crates/oracle-adapter`) and transport overlays fall under the same supply-chain gates: refresh `cargo vendor`, regenerate `provenance.json`/`checksums.txt`, attach fuzz coverage summaries, and document the attestation bundle in every release checklist (per `AGENTS.md §§15.F, 15.I`). Release tooling refuses tags when these artifacts drift.

## Data Retention and Privacy Compliance
- Privacy compliance from the old docs now lives here: reads store signatures only, storage manifests encrypt content keys, telemetry scrubs PII and includes sampling controls (`node/src/telemetry.rs`).
- Gateway caches encrypt at rest; even mobile caches derive keys from `TB_MOBILE_CACHE_KEY_HEX`/`TB_NODE_KEY_HEX` to avoid plaintext recoveries.
- Jurisdiction packs dictate retention timers. Governance votes log pack hashes so explorers/CLI can prove which policy was active for any block.

## Auditing and Tooling
- Settlement audits (`tools/settlement_audit`), dependency audits (`just dependency-audit`), and TLS warning snapshots all land in the aggregator for historical replay.
- Probe CLI can emit Prometheus metrics for latency SLAs; dashboards include authn/authz traces for RPC + gateway endpoints.
- Formal proofs, fuzz coverage, and chaos traces (bridge/compute/gossip) are expected before every release; see `docs/developer_handbook.md#formal-methods`.
