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

## Hardware Wallet Support

Mock Ledger/Trezor devices implement the `WalletSigner` trait. Tests under `node/tests` verify ed25519 and post-quantum flows with deterministic transcripts and error handling.

Physical hardware wallet support is planned but follows the same trait
interfaces. Until then, the mock implementations help developers exercise the
APIs without specialized equipment.

## Key Management Guides

- Seeds are stored in `$TB_WALLET_DIR` (defaults to `~/.the-block/wallets`).
- Accounts derive paths `m/44'/9000'/0'/0/<index>` and output Bech32 addresses.
- `wallet export --name alice` emits a JSON keystore; `wallet import` restores it.
- For air-gapped setups, use `wallet sign --stdin` to sign transactions offline
and `wallet submit` on an online machine. The CLI prints transaction hashes so
operators can confirm inclusion via the explorer.

Progress: 60%
