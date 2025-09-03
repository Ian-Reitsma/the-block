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
After restart, verify that governance parameters reflect the CT-only subsidy model by running `blockctl rpc inflation.params` to print `beta`, `gamma`, `kappa`, `lambda`, and `rent_rate_ct_per_byte`, then query stake balances with `blockctl rpc stake.role <provider>` to confirm that subsidy counters and role bonds carried over correctly.

For additional assurance, query `subsidy_bytes_total{type}` and `rent_escrow_locked_ct_total`
from the Prometheus endpoint and compare them against pre-upgrade snapshots. Any
unexpected jumps suggest lingering credit-era files or misapplied configs.

## Migrating from subsidy-ledger devnets

Legacy devnets stored a `credits.db` ledger beside the chain state. Remove it before
starting a CT-only node. The helper below validates the datadir path, prints the
target it is inspecting, and reports whether a ledger was removed:

```bash
scripts/zero_credits_db.sh ~/.block/datadir
# example output:
# checking /home/user/.block/credits.db
# removed legacy ledger
```

Genesis files no longer include `initial_credit_balances`; faucets dispense liquid
CT instead of the old credits. Use the helper script to top up accounts on test networks:

```bash
scripts/devnet_faucet.sh <address> [amount_nct]
```

For fresh deployments, seed accounts using the CT-only genesis template in
`examples/genesis/genesis.json` and point your node to this file at startup.

If migrating a fleet, roll out upgrades in waves and monitor the first upgraded
node for at least one epoch to ensure subsidy gauges increment and that
`governance/history` records the expected multiplier entries. Only after
confirmation should the remaining nodes be switched over.

