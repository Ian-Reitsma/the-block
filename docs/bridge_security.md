# Bridge Security Runbook
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The bridge enforces multi-signature relayer approvals and a challenge period for
withdrawals. Deposits must include a bundle of relayer proofs that meets the
configured quorum (`BridgeConfig::relayer_quorum`, default 2). Each proof is
validated independently; invalid signers are slashed and recorded via the
`bridge_slashes_total` counter.

Withdrawals create a pending record keyed by the aggregate commitment of the
relayer bundle. Funds remain reserved until the challenge window elapses. Any
operator may submit a dispute via `bridge.challenge_withdrawal`, which re-credits
the locked balance and slashes the approving relayers. Challenge activity is
tracked in both telemetry (`bridge_challenges_total`) and the explorer's
`bridge_challenges` table.

To finalize a withdrawal, call `bridge.finalize_withdrawal` after the configured
`challenge_period_secs`. The RPC returns an error until the deadline has passed
and no challenge has been filed. Explorer dashboards can surface active pending
withdrawals using `Explorer::active_bridge_challenges`, enabling operators to
identify commitments approaching expiry.

Simulations under `sim/bridge_threats.rs` explore delay scenarios and double
signing attempts. Extend these experiments when adjusting the quorum or
challenge duration to ensure deterministic dispute resolution across networks.