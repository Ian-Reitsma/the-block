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
hash and pressing the confirm button. A screenshot of the approval screen is
shown below for reference:

![HID approval prompt](assets/hid_approval.png)

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
transactions without exposing private keys. The service exposes two HTTP
endpoints:

- `GET /pubkey` returning `{ "pubkey": "<hex>" }`
- `POST /sign` accepting `{ "trace": "<uuid>", "msg": "<hex>" }`

Both the `sign` and `stake-role` CLI commands accept a `--remote-signer <url>`
flag that overrides `--seed`. Each request prefixes the `REMOTE_SIGN|` domain
tag, logs the unique trace ID, and retries on timeout. Example usage:

```bash
cargo run --bin wallet sign --message "hello" --remote-signer http://127.0.0.1:8000
```

Deploy remote signers on trusted networks and transport over TLS or USB to
avoid MITM attacks.

## Key Management Guides

- Seeds are stored in `$TB_WALLET_DIR` (defaults to `~/.the-block/wallets`).
- Accounts derive paths `m/44'/9000'/0'/0/<index>` and output Bech32 addresses.
- `wallet export --name alice` emits a JSON keystore; `wallet import` restores it.
- For air-gapped setups, use `wallet sign --stdin` to sign transactions offline
and `wallet submit` on an online machine. The CLI prints transaction hashes so
operators can confirm inclusion via the explorer.

Progress: 70%
