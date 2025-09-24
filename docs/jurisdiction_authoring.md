# Jurisdiction Policy Authoring
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Policy packs define consent requirements and feature toggles for a region. Packs may inherit from parent regions to model country/state/municipality hierarchies.

## Signing
Policy packs distributed over the network should be signed with an Ed25519 key controlled by governance. Nodes verify signatures before applying updates.

## Structure
```
{
  "region": "US-CA-SF",
  "parent": "US-CA",
  "consent_required": true,
  "features": ["wallet", "dex"]
}
```

Use `tools/jurisdiction_check.rs` to validate packs before publishing.