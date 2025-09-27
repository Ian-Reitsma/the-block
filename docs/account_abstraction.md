# Account Abstraction and Session Keys
> **Review (2025-09-25):** Synced Account Abstraction and Session Keys guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The node supports pluggable account validation via **session keys**. Accounts may
register ephemeral keys that authorize meta-transactions without exposing the
primary signing key.

## Session Keys

* Session keys are ordinary Ed25519 keys paired with an expiration timestamp.
* Keys are issued to an account through `issue_session_key` and stored in the
  account state with the most recent nonce used.
* During mempool admission each transaction is checked against the issuer's
  policies. Issuance increments `session_key_issued_total`; expired keys are
  rejected and counted by `session_key_expired_total`. Nonces must increase
  monotonically per session to prevent replay.

## Wallet Workflow

The CLI can derive session keys and sign meta-transactions:

```bash
contract wallet session --ttl 600      # prints session key and secret
contract wallet meta-send --to bob --amount 5 --session-sk <hex-secret>
```

## Security Notes

Session keys should be treated as sensitive secrets. They are intended for short
lived delegation and must be rotated frequently. Expired keys are automatically
invalidated but remain in the account history for audit purposes.

The simulator (`sim/src/lib.rs`) includes a session-key churn knob to stress
test issuance and expiration under load.
