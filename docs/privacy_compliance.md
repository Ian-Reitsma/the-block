# Privacy Compliance
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The node supports privacy-preserving features that allow operators to satisfy
jurisdictional regulations while minimizing data collection.

## Selective Disclosure
Transactions may redact memo fields when policies forbid storing
personally identifiable information. The `privacy_sanitization_total` metric
tracks such redactions.

## Differential Telemetry
Operators can enable differential privacy for metric emission to obscure exact
values while retaining aggregate trends.

## Law-Enforcement Requests
The LE portal accepts partial subpoenas and returns cryptographic proofs of
compliance, ensuring that only authorized data is revealed.