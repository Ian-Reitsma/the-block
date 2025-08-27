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
