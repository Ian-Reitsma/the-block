# Distributed Key Generation
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The DKG module implements a basic FROST-style protocol for deriving shared
validator keys. Nodes run DKG rounds to produce an aggregate public key while
keeping individual shares. Blocks may be threshold-signed using shares from a
quorum of validators.

Metrics:
- `dkg_round_total` tracks completed rounds.
- `threshold_signature_fail_total` counts failed verifications.

The system retains backward compatibility with single-signer blocks; threshold
verification is attempted only when a combined signature is present.