# Licensing, Trademark, and Canonical Release Policy

This repository (`the-block-core`) remains the open, consensus-critical implementation of The Block. Chain-critical code stays permissive (Apache 2.0) so validators, exchanges, and auditors can run and verify the network without fear of lock-in. A separate proprietary product (`the-block-enterprise`) packages operational extras for commercial buyers.

## Open Core (the-block-core)
- **License:** Apache 2.0 (or Apache/MIT dual where already used). No future restrictive covenants on consensus paths.
- **Scope:** consensus (`node/src/consensus`, `poh/`), networking/overlay (`node/src/net`, `crates/transport`, `crates/httpd`, `p2p_overlay/`), ledger/state (`ledger/`, `state/`, `node/src/blockchain`), VM (`node/src/vm`), receipts and validation (`node/src/receipts*`, `node/src/receipts_validation.rs`, `node/src/telemetry/receipts.rs`), governance rules (`governance/`, `node/src/governance`, `cli/src/governance`), protocol economics (`node/src/economics`, `docs/economics_and_governance.md`), node runtime + RPC/CLI/explorer surfaces that validators and exchanges rely on (`node/`, `cli/`, `explorer/`), and consensus-critical light clients/bridges.
- **Guardrails:** No BSL, source-available, or delayed-open licenses may apply to the above surfaces. Any feature that validators, explorers, or exchanges must run stays under Apache 2.0 with full source availability and reproducible builds.

## Enterprise Stack (the-block-enterprise)
- **License:** Commercial EULA (proprietary). Optional BSL 1.1 with an 18-36 month conversion window is permissible only for non-consensus enterprise modules.
- **Scope:** managed validator orchestration (K8s operators, auto-upgrade/rollback tooling), enterprise observability suite (dashboards, alerting, incident workflows, forensic/replay UI), compliance modules (policy controls, audit/reporting pipelines), hosted marketplace coordination/SLA services, premium SDKs + integration adapters + one-click infra deployments, and "operator-proof" automation that hardens production rollouts.
- **Integration contract:** Enterprise code must consume the open-core APIs/telemetry/governance surfaces without redefining consensus. Anything that alters block production, validation, fee rules, or ledger state belongs in the open core.

## Contributor License Agreement (CLA) + DCO
- **Inbound = outbound:** Contributions to this repository are accepted under Apache 2.0; contributors grant the maintainers the right to relicense their contributions for the proprietary enterprise stack while keeping the core permissive. See `CLA.md` for the full grant and representations.
- **Patent grant:** Contributors grant a patent license consistent with Apache 2.0 so downstream operators are protected.
- **Sign-off required:** Every commit must include `Signed-off-by:` (Developer Certificate of Origin). CI hooks (`scripts/check_cla.sh`, `node/tests/cla_check.rs`) enforce the trailer.
- **Acceptance flow:** Before a first contribution is merged, the author must agree to `CLA.md` (recorded via PR comment/portal) and use `git commit -s`. Entity contributors should note their organization in the sign-off line.

## Trademark & "Official Chain" Policy
- **Marks:** "The Block" name/logo/marks may not be used for forks or derivatives without written permission. Forks must rename to avoid confusion. (Trademark application submitted; placeholder until registration completes.)
- **Official releases:** Canonical builds are the ones signed by the maintainer keys and validated by the provenance pipeline (`provenance.json`, `checksums.txt`, `config/release_signers.txt`, and the `verify_release_signature` path described in `docs/security_and_privacy.md#release-provenance-and-supply-chain`). Unsigned binaries are non-canonical.
- **Reproducibility:** Releases must remain reproducible; publish build instructions and hashes so exchanges/operators can verify that signed artifacts match source. Any change to the signing keys or policy must be recorded in this document and `docs/security_and_privacy.md`.
- **Policy docs:** See `TRADEMARK.md` and `docs/official_chain_policy.md` for the full trademark and official chain rules.

## Operational Next Steps
- Publish the proprietary `the-block-enterprise` repository with its EULA and keep consensus-critical code in this Apache-licensed core. Location: `~/projects/the-block-enterprise` (sibling repo; not a submodule of `the-block-core`).
- Enforce CLA acceptance in PR templates/automation and maintain a ledger of approved contributors tied to their `Signed-off-by` identity.
- Document release-signing verification steps alongside the signing key fingerprints in `docs/security_and_privacy.md` and operator runbooks so "official chain" validation is turnkey for exchanges and validators.
