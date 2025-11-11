# Security and Privacy

Security is enforced in code, not promises. This guide consolidates the former threat-model, bridge-security, privacy, and supply-chain docs.

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
- Remote signer workflows live in `node/src/remote_signer_security.rs`, `cli/src/wallet.rs`, and `wallet/` crates. CLI enforces multisig, escrow-hash selection, and remote telemetry.
- Release provenance (`node/src/provenance.rs`) verifies binary hashes against signed allow lists; attested binaries roll back automatically if hashes drift.
- Environment variables `TB_RELEASE_SIGNERS`, `TB_RELEASE_SIGNERS_FILE` override defaults for air-gapped deployments.

## Privacy Layers
- Reads stay free by logging signed `ReadAck` receipts, not payloads. Operators can redact metadata via the privacy crate (`privacy/`) when the `privacy` feature is enabled.
- Read-ack privacy modes (`node/src/config.rs::ReadAckPrivacyMode` + `node/src/blockchain/privacy.rs`):
  - `Enforce` (default) — every receipt must include a privacy proof; failures raise `ReadAckError::PrivacyProofRejected`.
  - `Observe` — proofs are checked but failures are logged (`read_ack_privacy_verification_failed`) instead of rejected so operators can collect samples without losing revenue.
  - `Disabled` — privacy proof checks are skipped (only use during incident response). RPC `node.get_ack_privacy`/`node.set_ack_privacy` change modes live; CLI wrappers should restore `enforce` after drills.
- Law-enforcement portal (`node/src/le_portal.rs`) writes hashed case IDs and action logs; optional ChaCha20-Poly1305 evidence buckets live under `<base>/evidence/`.
- Range-boost mesh encrypts payloads, tracks hop proofs, and never exposes raw content to intermediate peers.

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
  - `LeRequest { timestamp, agency, case_hash, jurisdiction, language }` and `LeAction { action_hash, … }` are serialized to JSON and appended to `<base>/le_requests.log` and `<base>/le_actions.log`. CLI commands (`tb-cli le request|action`) accept `--base <dir>`; default base is `./le_portal`.
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

## Release Provenance and Supply Chain
- Release provenance is enforced by `node/src/provenance.rs`, `config/release_signers.txt`, and the CI job that verifies `provenance.json` + `checksums.txt`.
- Dependency independence: first-party wrappers (`foundation_*` crates) replace third-party TLS/HTTP/serialization stacks. `docs/developer_handbook.md#dependency-policy` covers required tooling and audits.
- Reproducible builds: `docs/repro.md` + `docs/reproducible_builds.md` were merged here. Build IDs must match `env!("BUILD_BIN_HASH")` or binaries are rejected on startup.

## Data Retention and Privacy Compliance
- Privacy compliance from the old docs now lives here: reads store signatures only, storage manifests encrypt content keys, telemetry scrubs PII and includes sampling controls (`node/src/telemetry.rs`).
- Gateway caches encrypt at rest; even mobile caches derive keys from `TB_MOBILE_CACHE_KEY_HEX`/`TB_NODE_KEY_HEX` to avoid plaintext recoveries.
- Jurisdiction packs dictate retention timers. Governance votes log pack hashes so explorers/CLI can prove which policy was active for any block.

## Auditing and Tooling
- Settlement audits (`tools/settlement_audit`), dependency audits (`just dependency-audit`), and TLS warning snapshots all land in the aggregator for historical replay.
- Probe CLI can emit Prometheus metrics for latency SLAs; dashboards include authn/authz traces for RPC + gateway endpoints.
- Formal proofs, fuzz coverage, and chaos traces (bridge/compute/gossip) are expected before every release; see `docs/developer_handbook.md#formal-methods`.
