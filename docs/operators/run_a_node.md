# Run a node

## Hardware
- 4 cores
- 8 GB RAM
- SSD with at least 50 GB free

## Ports
- P2P: `3033` (TCP and UDP if using QUIC)
- RPC: `3030`
- Metrics: `9898`

## Quickstart
```sh
curl -LO <release-tar>
./scripts/verify_release.sh node-<ver>-x86_64.tar.gz checksums.txt checksums.txt.sig
mkdir -p ~/.block
 tar -xzf node-<ver>-x86_64.tar.gz -C ~/.block
~/.block/node --datadir ~/.block/datadir --config ~/.block/config.toml
```

### systemd
Create `/etc/systemd/system/the-block.service`:
```ini
[Unit]
Description=The Block node
After=network.target

[Service]
ExecStart=/home/user/.block/node --datadir /home/user/.block/datadir --config /home/user/.block/config.toml
Restart=always
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```
Enable and start:
```sh
systemctl enable --now the-block
```

### Firewall
Allow P2P and metrics if required; restrict RPC to localhost.
Run the node with `--metrics-addr` and `--features telemetry` to surface
`read_denied_total` and `credit_issued_total` counters for monitoring.
