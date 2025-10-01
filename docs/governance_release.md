# Governance-Secured Release Flow
> **Review (2025-09-30):** Documented overlay peer-store migration helper in release prep checklist.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the process for approving and installing node
software releases through on-chain governance. Nodes may only install
binaries whose build hashes have been endorsed by token holders and
attested by a quorum of release signers.

1. A release build is produced and its hash is signed by the build
   infrastructure using one or more Ed25519 attestor keys.
2. Validators submit a `ReleaseVote` proposal referencing the signed
   hash. The proposal encodes the attesting signer set and the required
   signature threshold. Once the proposal passes, the governance
   controller publishes the hash on chain together with the quorum data.
3. During startup, nodes fetch the list of approved hashes and refuse
   to run binaries that are not present.
4. Release provenance now records the staged vendor tree hash and the dependency
   snapshot referenced in governance proposals. CI refuses to publish tags until
   `scripts/release_provenance.sh` emits matching digests, and `scripts/verify_release.sh`
   confirms that the shipped snapshot aligns with the committed policy baseline. Governance
   proposals should include the vendor digest and snapshot path so operators can cross-check
   artifacts before voting.

Prior to rolling out a release that upgrades the in-house overlay backend, run the bundled
peer-store migrator so persisted peer IDs land in the new canonical path. Nodes that still
store peers in `~/.the_block/overlay_peers.json` should execute:

```
cargo run --bin migrate_overlay_store --release -- \
  ~/.the_block/overlay_peers.json ~/.the_block/overlay/peers.json
```

For bespoke layouts, supply explicit source and destination paths:

```
cargo run --bin migrate_overlay_store --release -- \
  /srv/the-block/overlay_peers.json /var/lib/the-block/overlay/peers.json
```

The helper canonicalises every peer ID to base58-check form and stamps the migration timestamp
so uptime probes immediately recognise the refreshed entries. See
[`docs/operators/run_a_node.md`](operators/run_a_node.md#migrating-overlay-peer-stores) for
additional validation commands.

Wallets expose two helper commands:

- `governance propose-release <hash>` creates a `ReleaseVote` proposal.
- `governance approve-release <proposal-id>` votes in favour of the
  referenced hash.

The CLI now surfaces these flows via:

```
contract gov propose-release --hash <BLAKE3> \
    --signature <SIG1> --signer <PUBKEY1> \
    [--signature <SIG2> --signer <PUBKEY2> ...] \
    [--threshold <N>]
contract gov approve-release --id <ID>
```

The CLI enforces that each `--signer` corresponds to a provided
signature. If the threshold is omitted it defaults to the current
signer set size, guaranteeing a full quorum. The `release_quorum_fail_total`
metric increments whenever a submission lacks sufficient attestations.
Operators can query the current signer roster via the JSON-RPC method
`gov.release_signers` which returns the active signer list and the most
recently applied threshold.

Nodes refuse to boot unless the compile-time `BUILD_BIN_HASH` appears in
the governance store. Successful boots increment `release_installs_total`
allowing operators to confirm cluster-wide adoption.

## Provenance signatures

Release proposals may require a provenance signature before they can be
submitted. Operators configure trusted signer public keys via either the
`TB_RELEASE_SIGNERS` environment variable (comma or newline separated
hex-encoded Ed25519 keys) or by populating `config/release_signers.txt`.
An optional `TB_RELEASE_SIGNERS_FILE` can point to an alternate file.

When any signer is configured, `ReleaseVote` submissions must include
signatures from those signers. Each signature is an Ed25519 signature
over `"release:<hash>"`. Invalid or missing signatures are rejected
before the proposal is persisted, ensuring the on-chain list only
contains hashes backed by trusted build provenance. The on-chain
records preserve the signer snapshot and required threshold for future
audits.

Governance votes for releases are exposed through the
`release_votes_total` counter, enabling dashboards to distinguish
between tentative and activated builds. `release_installs_total`
surfaces node uptake, while `release_quorum_fail_total` highlights
proposals that failed attestation quorum.

## Automatic fetch and verification

Operators can retrieve release artifacts directly from the configured
distribution source using the built-in update helper:

```
contract gov fetch-release --hash <BLAKE3> \
    [--signature <SIG> --signer <PUBKEY> ...] \
    [--dest ./downloads/<HASH>.bin] [--install]
```

The command downloads `<BLAKE3>.bin` from
`$TB_RELEASE_SOURCE_URL/<HASH>.bin`, verifies the BLAKE3 digest, and
optionally checks the provided signatures with the attestor keys. When
`--install` is supplied the CLI calls
`the_block::update::install_release`, which in turn invokes
`ensure_release_authorized` so the install is recorded via
`release_installs_total`. Startup failures automatically trigger
`update::rollback_failed_startup`, which restores the previous binary
from `$TB_PREVIOUS_BINARY` if present.

Programmatic consumers can reuse `the_block::update::fetch_release`
which returns a `DownloadedRelease` containing the verified bytes and
the staging path.

## Explorer timeline API

The explorer exposes a paginated release timeline at `GET /releases`.
Query parameters:

- `page`/`page_size` – zero-based pagination controls.
- `proposer` – filter by proposer address (case-insensitive).
- `start_epoch`/`end_epoch` – inclusive activation epoch range.
- `store` – optional path override for the governance database.

Responses follow the schema in `explorer/api_schema/release_history.json`
and include full attestation metadata, installer counts, and per-node
install timestamps. The handler caches results for 15 seconds to reduce
database load and persists proposer/activation metadata to the
`release_history` table.

Operators can query the same data from the CLI:

```
contract explorer release-history --state governance_db \
    [--proposer <ADDR>] [--start-epoch <E>] [--end-epoch <E>] \
    [--page N] [--page-size M]
```

The explorer crate also exposes
`Explorer::release_timeline(&gov_path)` for rendering charts of install
counts over time.

Explorer operators can surface an approval timeline via the
`release_view::release_history` helper, which reads directly from the
governance store and lists each approved hash alongside the proposer,
activation epoch, and the latest installation timestamp observed across
the fleet. Rows also display the most recent quorum evaluation so that
missing signatures or threshold shortfalls are visible alongside each
entry.

## Simulations and slow-path validation

Two simulator entry points exercise the release workflow under stress:

- `sim/release_signers.rs` randomises signer churn, quorum thresholds, and
  approval cadence to ensure that governance tolerates membership changes.
- `sim/lagging_release.rs` injects delayed fetches and slow hash verification
  to validate rollback behaviour and confirm that install timestamps remain
  monotonic even under partial failures.

Operators should run these simulations when adjusting thresholds or
introducing new fetch mirrors to confirm the ecosystem-wide blast radius
before deploying changes.

## Rollback drills

Operators can rehearse rollback procedures offline:

1. Fetch a known-good release with `contract gov fetch-release --hash <OLD> --dest ./staging/<OLD>.bin`.
2. Stage a deliberately corrupted binary in the live path and restart the node; startup will fail provenance validation and call `update::rollback_failed_startup`.
3. Confirm the node restored the previous binary and wrote an audit log identifying the rolled-back hash without bumping `release_installs_total` for the rejected build.
4. Reinstall the current release with `--install` so `release_installs_total` reflects the return to service.

Documenting the outcome of these drills in change-management systems ensures that multi-signer governance remains enforceable even when a release needs to be revoked under pressure.

## Release Checklist

- Verify `the-block net overlay-status` on staging nodes before tagging a release
  and confirm the reported backend and peer database path match the planned
  configuration.
- Confirm Grafana dashboards display non-zero `overlay_peer_total{backend}` and
  `overlay_peer_persisted_total{backend}` values for the active backend so fleet
  telemetry agrees with the staging sanity check.
- Capture the overlay backend selection in the release notes so operators can
  audit that production nodes match the staged configuration.
- Run `cargo run -p release_notes -- --state-dir /var/lib/the-block` (or point
  `--history` at an exported archive) to summarise any runtime, transport, or
  storage backend switches approved during the cycle; paste the output into the
  release notes alongside the dependency snapshot digests so operators can
  confirm governance intent before upgrading.
