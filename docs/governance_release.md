# Governance-Secured Release Flow

This document outlines the process for approving and installing node
software releases through on-chain governance.  Nodes may only install
binaries whose build hashes have been endorsed by token holders.

1. A release build is produced and its hash is signed by the build
   infrastructure using an Ed25519 attestor key.
2. Validators submit a `ReleaseVote` proposal referencing the signed
   hash.  Once the proposal passes, the governance controller publishes
   the hash on chain.
3. During startup, nodes fetch the list of approved hashes and refuse
   to run binaries that are not present.

Wallets expose two helper commands:

- `governance propose-release <hash>` creates a `ReleaseVote` proposal.
- `governance approve-release <proposal-id>` votes in favour of the
  referenced hash.

The CLI now surfaces these flows via:

```
contract gov propose-release --hash <BLAKE3> [--signature SIG]
contract gov approve-release --id <ID>
```

Nodes refuse to boot unless the compile-time `BUILD_BIN_HASH` appears in
the governance store. Successful boots increment `release_installs_total`
allowing operators to confirm cluster-wide adoption.

## Provenance signatures

Release proposals may require a provenance signature before they can be
submitted. Operators configure trusted signer public keys via either the
`TB_RELEASE_SIGNERS` environment variable (comma or newline separated
hex-encoded Ed25519 keys) or by populating `config/release_signers.txt`.
An optional `TB_RELEASE_SIGNERS_FILE` can point to an alternate file.

When any signer is configured, `ReleaseVote` submissions must include a
`--signature` argument containing the attestor’s Ed25519 signature over
`"release:<hash>"`. Invalid or missing signatures are rejected before
the proposal is persisted, ensuring the on-chain list only contains
hashes backed by trusted build provenance.

Governance votes for releases are exposed through the `release_votes_total`
counter, enabling dashboards to distinguish between tentative and activated
builds, while `release_installs_total` surfaces node uptake.

Explorer operators can surface an approval timeline via the
`release_view::release_history` helper, which reads directly from the
governance store and lists each approved hash alongside the proposer,
activation epoch, and the latest installation timestamp observed across
the fleet.
