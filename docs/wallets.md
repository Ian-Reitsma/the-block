# Wallets

The workspace provides a modular wallet framework for key management and signing.

## Wallet Crate

`crates/wallet` implements seed-based account derivation (ed25519) and a pluggable `WalletSigner` trait. Deterministic tests cover key derivation and round-trip signing.

## CLI

`node/src/bin/wallet.rs` exposes a CLI for generating keys, staking roles, and
signing messages. Signers can be software, hardware (via HID/WebUSB mock), or
remote.

Examples:

```bash
cargo run --bin wallet generate
cargo run --bin wallet sign --seed <hex> --message "hello"
cargo run --bin wallet stake-role gateway 100 --seed <hex>
```

The `stake-role` command builds and submits a staking transaction that bonds CT
for a specific service role. Use `--withdraw` to unbond and retrieve stake after
the unbonding period. To inspect rent-escrow balances associated with your
account, run:

```bash
cargo run --bin wallet escrow-balance <account>
```

This prints total CT locked for active blobs so operators can gauge exposure
before pruning storage.

## Session Keys

Ephemeral session keys can authorize meta-transactions without exposing the
root seed. Issue a key with a time-to-live and use it for delegated sends:

```bash
cargo run --bin wallet session --ttl 600
cargo run --bin wallet meta-send --to bob --amount 5 --session-sk <hex>
```

Session activity surfaces via `session_key_issued_total` and
`session_key_expired_total` metrics.

## Delegation and Reward Withdrawal

Delegation transactions transfer voting power to a validator while keeping
tokens in the delegator's account. The payload contains:

```text
{ delegator: <addr>, validator: <addr>, amount: u64, nonce: u64 }
```

The delegator signs the above message with Ed25519; the validator's signature is
optional but recommended for mutual authorization. Nonces must be sequential and
match the on-chain account state or the transaction is rejected.

Withdrawals reclaim accumulated rewards and unbonded stake. A withdrawal message
is structurally similar:

```text
{ delegator: <addr>, validator: <addr>, withdraw: true, nonce: u64 }
```

The wallet increments the nonce and signs with the same key used for delegation.
Funds are unlocked once the unbonding period elapses.

### CLI Examples

```bash
# delegate 250 CT to validator bob
wallet delegate --to bob --amount 250 --seed <hex>

# withdraw all mature rewards
wallet withdraw --from alice --seed <hex>
```

Both commands print the transaction hash and update a local JSON log for audit
purposes. Failed submissions include a reason field pointing to nonce mismatches
or insufficient balance.

### Hardware Wallet Flow

When a Ledger/Trezor device is connected, the CLI prompts on the hardware
screen before broadcasting. Approve the transaction by matching the displayed
hash and pressing the confirm button.

### Monitoring

Telemetry counters track staking operations:

- `delegation_total{validator}` – successful delegation transactions.
- `withdraw_total{validator}` – reward withdrawals.

Expose these via `--metrics-addr` and set alerts when unexpected spikes occur.

## Staking Advice

`crates/wallet::stake` exposes `stake_advice` which computes a Kelly-optimal stake
fraction and CVaR$_{0.999}$ using a Cornish–Fisher corrected $
\sigma$.
Inputs include block win probability, reward, slash loss, return volatility,
skew and kurtosis. The function returns `(f^*, \text{CVaR}_{0.999})` where
`f^*` is the recommended fraction of liquid CT to bond.

## Hardware Wallet Support

Ledger Nano S/X and Trezor T devices implement the `WalletSigner` trait via the
`wallet-hw` feature. Integration tests under `node/tests` verify Ed25519 and
post‑quantum flows with deterministic transcripts and error handling, allowing
developers to exercise the same APIs against real hardware.

### Gaps

- Automated firmware update workflows
- Broader vendor coverage beyond Ledger and Trezor

## Remote Signer

Remote signer daemons let hardware devices or offline machines approve
transactions without exposing private keys.  Signers advertise themselves via
UDP broadcast (`wallet::remote_signer::discover`) and can be reached over HTTPS
or mutually authenticated WebSockets.  Each signer exposes a `GET /pubkey`
endpoint returning `{ "pubkey": "<hex>" }` and a `/sign` method over HTTP or
WSS that accepts `{ "trace": "<uuid>", "msg": "<hex>" }`.

Public keys are cached for 10 minutes to avoid repeated round‑trips.  Requests
increment `remote_signer_request_total` and any failures increment
`remote_signer_error_total{reason}`.

The `sign` and `stake-role` CLI commands accept one or more
`--remote-signer <url>` flags and a `--threshold <n>` option.  The wallet
collects signatures until the threshold is met and concatenates them for
multisig transactions. For WSS endpoints that require mutual TLS, supply
`--signer-cert <pem>` and `--signer-key <pem>` to present a client
certificate, and `--signer-ca <pem>` when the signer uses a non-standard
certificate authority. These map to the `REMOTE_SIGNER_TLS_CERT`,
`REMOTE_SIGNER_TLS_KEY`, and `REMOTE_SIGNER_TLS_CA` environment variables.
Example usage:

```bash
cargo run --bin wallet sign --message "hello" \
    --remote-signer http://127.0.0.1:8000 \
    --remote-signer http://127.0.0.1:8001 --threshold 2
```

Deploy remote signers on trusted networks and prefer the secure WSS transport
to avoid MITM attacks.

## Key Management Guides

- Seeds are stored in `$TB_WALLET_DIR` (defaults to `~/.the-block/wallets`).
- Accounts derive paths `m/44'/9000'/0'/0/<index>` and output Bech32 addresses.
- `wallet export --name alice` emits a JSON keystore; `wallet import` restores it.
- For air-gapped setups, use `wallet sign --stdin` to sign transactions offline
and `wallet submit` on an online machine. The CLI prints transaction hashes so
operators can confirm inclusion via the explorer.

Progress: 70%
