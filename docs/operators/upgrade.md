# Upgrade a node

1. Download new release and verify:
```sh
curl -LO <release-tar>
./scripts/verify_release.sh node-<ver>-x86_64.tar.gz checksums.txt checksums.txt.sig
```
2. Prepare blue/green symlink:
```sh
mkdir -p ~/bin
ln -sfn ~/releases/node-<ver> ~/bin/node-next
```
3. Stop node:
```sh
systemctl stop the-block
```
4. Swap binary:
```sh
ln -sfn ~/bin/node-next ~/.block/node
```
5. Start node:
```sh
systemctl start the-block
```
Datadirs remain backward compatible; take a backup of `~/.block/datadir` before upgrading.
After restart, verify governance parameters and that `read_reward_pool` is seeded by running `blockctl gov params` and `blockctl wallet balance <provider>`.

## Migrating from credit-based devnets

Legacy devnets stored a `credits.db` ledger beside the chain state. Remove it before
starting a CT-only node:

```bash
scripts/zero_credits_db.sh ~/.block/datadir
```

Genesis files no longer include `initial_credit_balances`; faucets dispense liquid
CT instead of credits. Use the helper script to top up accounts on test networks:

```bash
scripts/devnet_faucet.sh <address> [amount_nct]
```

For fresh deployments, seed accounts using the CT-only genesis template in
`examples/genesis/genesis.json` and point your node to this file at startup.

