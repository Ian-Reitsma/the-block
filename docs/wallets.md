# Wallets

The workspace provides a modular wallet framework for key management and signing.

## Wallet Crate

`crates/wallet` implements seed-based account derivation (ed25519) and a pluggable `WalletSigner` trait. Deterministic tests cover key derivation and round-trip signing.

## CLI

`node/src/bin/wallet.rs` exposes a CLI for generating keys, listing accounts, and signing messages or transactions. Signers can be software, hardware (via HID/WebUSB mock), or remote.

Example:

```bash
cargo run --bin wallet generate --name alice
cargo run --bin wallet sign --name alice --message "hello"
```

## Hardware Wallet Support

Mock Ledger/Trezor devices implement the `WalletSigner` trait. Tests under `node/tests` verify ed25519 and post-quantum flows with deterministic transcripts and error handling.

## Key Management Guides

- Seeds are stored in `$TB_WALLET_DIR` (defaults to `~/.the-block/wallets`).
- Accounts derive paths `m/44'/9000'/0'/0/<index>` and output Bech32 addresses.
- `wallet export --name alice` emits a JSON keystore; `wallet import` restores it.

Progress: 60%
