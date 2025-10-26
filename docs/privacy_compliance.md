# Privacy Compliance
> **Review (2025-09-25):** Synced Privacy Compliance guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The node supports privacy-preserving features that allow operators to satisfy
jurisdictional regulations while minimizing data collection.

## Selective Disclosure
Transactions may redact memo fields when policies forbid storing
personally identifiable information. The `privacy_sanitization_total` metric
tracks such redactions.

## Differential Telemetry
Operators can enable differential privacy for metric emission to obscure exact
values while retaining aggregate trends.

## Zero-knowledge read acknowledgements
The gateway and node attach zero-knowledge proofs (`ReadinessPrivacyProof` and
`ReadAckPrivacyProof`) to every acknowledgement. The proofs demonstrate that
readiness thresholds were met and that acknowledgements are bound to anonymous
client commitments without leaking viewer identities. Enforcement is controlled
via `--ack-privacy` on the node CLI or the `node.{get,set}_ack_privacy` RPCs,
allowing operators to tighten or observe the policy while maintaining
compliance.

## Law-Enforcement Requests
The LE portal accepts partial subpoenas and returns cryptographic proofs of
compliance, ensuring that only authorized data is revealed.
