# Multi-Node Testing Setup Guide

**Target Configuration**: 1 PC + 2 Mac M1 Air

---

## Table of Contents

- [Overview](#overview)
- [Hardware Requirements](#hardware-requirements)
- [Network Setup](#network-setup)
- [Node Configuration](#node-configuration)
- [Dashboard Automation](#dashboard-automation)
- [Test Scenarios](#test-scenarios)
- [Monitoring Setup](#monitoring-setup)
- [Troubleshooting](#troubleshooting)

---

## Overview

This guide covers setting up a 3-node test cluster for The Block blockchain:

**Node Topology**:
```
PC (Primary)                Mac M1 Air #1 (Replica)      Mac M1 Air #2 (Observer)
─────────────              ────────────────────────      ───────────────────────
• Block production         • Treasury executor (hot standby)  • Metrics aggregator
• Treasury executor        • Validation node               • Dashboard host
• Full node sync           • Prometheus scraper            • Test coordinator
• Metrics exporter         • Metrics exporter              • Load generator
```

**Use Cases**:
1. **Consensus testing**: Multi-validator block production
2. **Failover testing**: Treasury executor redundancy
3. **Load testing**: Distributed stress test execution
4. **Network partitioning**: Simulate split-brain scenarios
5. **Performance benchmarking**: Real-world latency measurements

---

## Hardware Requirements

### PC (Primary Node)

- **CPU**: 4+ cores (8 threads)
- **RAM**: 16GB+
- **Storage**: 500GB+ SSD
- **Network**: Gigabit Ethernet
- **OS**: Ubuntu 22.04 LTS or later

### Mac M1 Air #1 (Replica)

- **CPU**: Apple M1 (8 cores)
- **RAM**: 8GB+ (16GB recommended)
- **Storage**: 256GB+ SSD
- **Network**: WiFi 6 or Ethernet adapter
- **OS**: macOS 12.0+

### Mac M1 Air #2 (Observer)

- **CPU**: Apple M1 (8 cores)
- **RAM**: 8GB+ (16GB recommended)
- **Storage**: 256GB+ SSD
- **Network**: WiFi 6 or Ethernet adapter
- **OS**: macOS 12.0+

### Network Requirements

- **Bandwidth**: 100 Mbps+ between nodes
- **Latency**: <10ms between nodes (same LAN)
- **Ports**: 9000-9010 (configurable)

---

## Network Setup

### Static IP Assignment

Configure static IPs for reliable inter-node communication:

```bash
# PC (Primary) - 192.168.1.10
# Mac M1 #1     - 192.168.1.11
# Mac M1 #2     - 192.168.1.12
```

#### PC (Ubuntu)

Edit `/etc/netplan/01-netcfg.yaml`:

```yaml
network:
  version: 2
  renderer: networkd
  ethernets:
    eth0:
      addresses:
        - 192.168.1.10/24
      gateway4: 192.168.1.1
      nameservers:
        addresses: [8.8.8.8, 8.8.4.4]
```

Apply:
```bash
sudo netplan apply
```

#### Mac M1 (both)

System Preferences → Network → Advanced → TCP/IP:
- Configure IPv4: Manually
- IP Address: 192.168.1.11 (or .12 for second Mac)
- Subnet Mask: 255.255.255.0
- Router: 192.168.1.1

### Firewall Configuration

#### PC (Ubuntu)

```bash
# Allow The Block ports (gossip + QUIC + RPC + telemetry)
sudo ufw allow 9000:9010/tcp
sudo ufw allow 9000:9010/udp

# Allow first-party telemetry + aggregator
sudo ufw allow 9898:9900/tcp   # metrics endpoints (per node)
sudo ufw allow 9000/tcp       # metrics-aggregator (observer role)

# Enable firewall
sudo ufw enable
```

#### Mac M1

System Preferences → Security & Privacy → Firewall → Firewall Options:
- Allow incoming connections for "the-block"
- Allow incoming connections for telemetry endpoint (`the-block` metrics) and metrics-aggregator (observer)

### Hosts File

Add to `/etc/hosts` on all nodes:

```
192.168.1.10    node-pc       primary
192.168.1.11    node-mac1     replica1
192.168.1.12    node-mac2     observer
```

---

## Node Configuration

### Installation (All Nodes)

- **Shortcut**: `scripts/multi-node/run-node.sh` (ROLE=primary|replica1|observer) bootstraps data dirs, RPC/metrics/QUIC ports, and enables range-boost discovery for LAN meshes. Run `scripts/multi-node/run-aggregator.sh` on the observer to expose the first-party telemetry feed.
- **Cluster test**: Once the three nodes are up, set `TB_MULTI_NODE_RPC=192.168.1.10:3030,192.168.1.11:4030,192.168.1.12:5030` (or your ports) and run `scripts/multi-node/run-cluster-tests.sh` to exercise the multi-node RPC smoke test (`node/tests/multi_node_rpc.rs`).

#### PC (Ubuntu)

```bash
# Install dependencies
sudo apt update
sudo apt install -y build-essential curl git

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone repository
git clone https://github.com/your-org/the-block.git
cd the-block

# Build (release mode for performance)
cargo build --release

# Install binary
sudo cp target/release/the-block /usr/local/bin/

# Create service user
sudo useradd -r -s /bin/false the-block

# Create directories
sudo mkdir -p /var/lib/the-block/{data,config,logs}
sudo chown -R the-block:the-block /var/lib/the-block
```

#### Mac M1 (both)

```bash
# Install Homebrew if not present
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/your-org/the-block.git
cd the-block
cargo build --release

# Install
sudo cp target/release/the-block /usr/local/bin/

# Create directories
sudo mkdir -p /usr/local/var/the-block/{data,config,logs}
sudo chown -R $(whoami):staff /usr/local/var/the-block
```

### Node-Specific Configuration

#### PC (Primary Node)

`/var/lib/the-block/config/the-block.toml`:

```toml
[network]
listen_addr = "0.0.0.0:9001"
external_addr = "192.168.1.10:9001"
bootstrap_nodes = []  # Primary bootstraps others

[node]
role = "validator"
identity = "primary-pc"

[treasury]
executor_enabled = true
executor_identity = "executor-primary"
lease_ttl = 60  # seconds
poll_interval = 10  # seconds

[telemetry]
enabled = true
prometheus_addr = "0.0.0.0:9100"
endpoint = "http://192.168.1.12:9090/api/v1/write"  # Push to Mac #2

[storage]
data_dir = "/var/lib/the-block/data"
cache_size_mb = 4096

[consensus]
validator_key = "/var/lib/the-block/config/validator.key"
```

#### Mac M1 #1 (Replica)

`/usr/local/var/the-block/config/the-block.toml`:

```toml
[network]
listen_addr = "0.0.0.0:9001"
external_addr = "192.168.1.11:9001"
bootstrap_nodes = ["192.168.1.10:9001"]

[node]
role = "validator"
identity = "replica-mac1"

[treasury]
executor_enabled = true  # Hot standby
executor_identity = "executor-replica1"
lease_ttl = 60
poll_interval = 10
standby_mode = true  # Only activate if primary fails

[telemetry]
enabled = true
prometheus_addr = "0.0.0.0:9100"
endpoint = "http://192.168.1.12:9090/api/v1/write"

[storage]
data_dir = "/usr/local/var/the-block/data"
cache_size_mb = 2048

[consensus]
validator_key = "/usr/local/var/the-block/config/validator.key"
```

#### Mac M1 #2 (Observer + Metrics)

`/usr/local/var/the-block/config/the-block.toml`:

```toml
[network]
listen_addr = "0.0.0.0:9001"
external_addr = "192.168.1.12:9001"
bootstrap_nodes = ["192.168.1.10:9001", "192.168.1.11:9001"]

[node]
role = "observer"  # Non-validator, read-only
identity = "observer-mac2"

[treasury]
executor_enabled = false  # Observer doesn't execute

[telemetry]
enabled = true
prometheus_addr = "0.0.0.0:9100"
# No endpoint - this node runs Prometheus server

[storage]
data_dir = "/usr/local/var/the-block/data"
cache_size_mb = 2048
```

### Systemd Service Files

#### PC - `/etc/systemd/system/the-block.service`

```ini
[Unit]
Description=The Block Node (Primary)
After=network.target

[Service]
Type=simple
User=the-block
Group=the-block
WorkingDirectory=/var/lib/the-block
ExecStart=/usr/local/bin/the-block run --config /var/lib/the-block/config/the-block.toml
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

#### Mac M1 - LaunchDaemon

Create `/Library/LaunchDaemons/com.theblock.node.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.theblock.node</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/the-block</string>
        <string>run</string>
        <string>--config</string>
        <string>/usr/local/var/the-block/config/the-block.toml</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/usr/local/var/the-block/logs/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/usr/local/var/the-block/logs/stderr.log</string>
</dict>
</plist>
```

Load:
```bash
sudo launchctl load /Library/LaunchDaemons/com.theblock.node.plist
```

---

## Dashboard Automation

### Automated Deployment Script

Create `/opt/the-block/scripts/deploy-dashboards.sh` on Mac M1 #2:

```bash
#!/bin/bash
# Automated dashboard deployment for multi-node cluster

set -e

MONITORING_DIR="/opt/the-block/monitoring"
GRAFANA_URL="http://localhost:3000"
GRAFANA_API_KEY="${GRAFANA_API_KEY:-admin:admin}"

echo "═══════════════════════════════════════"
echo "  The Block Dashboard Deployment"
echo "═══════════════════════════════════════"

# 1. Install Prometheus
if ! command -v prometheus &> /dev/null; then
    echo "Installing Prometheus..."
    brew install prometheus
fi

# 2. Install Grafana
if ! command -v grafana-server &> /dev/null; then
    echo "Installing Grafana..."
    brew install grafana
fi

# 3. Configure Prometheus
echo "Configuring Prometheus..."
cat > /usr/local/etc/prometheus.yml <<EOF
global:
  scrape_interval: 15s
  evaluation_interval: 15s

# Load recording and alerting rules
rule_files:
  - '$MONITORING_DIR/prometheus_recording_rules.yml'
  - '$MONITORING_DIR/alert_rules.yml'

# Alert manager configuration
alerting:
  alertmanagers:
    - static_configs:
        - targets: ['localhost:9093']

# Scrape configs for multi-node cluster
scrape_configs:
  # Primary node (PC)
  - job_name: 'node-primary'
    static_configs:
      - targets: ['192.168.1.10:9100']
        labels:
          instance: 'primary-pc'
          role: 'validator'

  # Replica node (Mac M1 #1)
  - job_name: 'node-replica1'
    static_configs:
      - targets: ['192.168.1.11:9100']
        labels:
          instance: 'replica-mac1'
          role: 'validator'

  # Observer node (Mac M1 #2 - local)
  - job_name: 'node-observer'
    static_configs:
      - targets: ['localhost:9100']
        labels:
          instance: 'observer-mac2'
          role: 'observer'

  # Treasury executors
  - job_name: 'treasury-executor'
    static_configs:
      - targets:
          - '192.168.1.10:9101'  # Primary
          - '192.168.1.11:9101'  # Replica
        labels:
          component: 'treasury'
EOF

# 4. Start Prometheus
echo "Starting Prometheus..."
brew services restart prometheus

# 5. Install AlertManager
echo "Installing AlertManager..."
brew install alertmanager

# 6. Configure AlertManager
cp "$MONITORING_DIR/alertmanager.yml" /usr/local/etc/alertmanager.yml

# 7. Start AlertManager
brew services restart alertmanager

# 8. Start Grafana
brew services restart grafana

# Wait for Grafana to start
sleep 5

# 9. Import dashboards
echo "Importing Grafana dashboards..."

for dashboard in "$MONITORING_DIR"/*.json; do
    echo "  - $(basename "$dashboard")"
    curl -X POST \
        -H "Authorization: Bearer $GRAFANA_API_KEY" \
        -H "Content-Type: application/json" \
        -d @"$dashboard" \
        "$GRAFANA_URL/api/dashboards/db"
done

# 10. Configure datasources
echo "Configuring Prometheus datasource..."
curl -X POST \
    -H "Authorization: Bearer $GRAFANA_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{
        "name": "Prometheus",
        "type": "prometheus",
        "url": "http://localhost:9090",
        "access": "proxy",
        "isDefault": true
    }' \
    "$GRAFANA_URL/api/datasources"

echo ""
echo "✅ Dashboard deployment complete!"
echo ""
echo "Access dashboards at:"
echo "  Grafana:        http://192.168.1.12:3000"
echo "  Prometheus:     http://192.168.1.12:9090"
echo "  AlertManager:   http://192.168.1.12:9093"
echo ""
echo "Default credentials:"
echo "  Username: admin"
echo "  Password: admin"
echo ""
```

Make executable:
```bash
chmod +x /opt/the-block/scripts/deploy-dashboards.sh
```

Run:
```bash
./opt/the-block/scripts/deploy-dashboards.sh
```

---

## Test Scenarios

### Scenario 1: Baseline Performance

**Objective**: Measure single-node performance

```bash
# On PC
cargo test --release treasury_extreme --ignored --test treasury_extreme_stress_test
```

Expected: 10k+ TPS on modern hardware

### Scenario 2: Multi-Node Consensus

**Objective**: Verify 3-node consensus

```bash
# On Mac M1 #2 (coordinator)
./scripts/test-consensus.sh --nodes=3 --duration=60s
```

Validates:
- All nodes agree on block height
- No forks observed
- Block production <5s

### Scenario 3: Treasury Failover

**Objective**: Test treasury executor failover

```bash
# 1. Verify primary is active
curl http://192.168.1.10:9100/metrics | grep treasury_executor_active
# Should return: treasury_executor_active{role="primary"} 1

# 2. Kill primary executor
ssh the-block@192.168.1.10 'systemctl stop the-block-treasury'

# 3. Wait for lease expiry (60s)
sleep 65

# 4. Verify replica took over
curl http://192.168.1.11:9100/metrics | grep treasury_executor_active
# Should return: treasury_executor_active{role="primary"} 1

# 5. Restore primary
ssh the-block@192.168.1.10 'systemctl start the-block-treasury'
```

### Scenario 4: Network Partition

**Objective**: Simulate split-brain

```bash
# On Mac M1 #2
./scripts/test-partition.sh --isolate=192.168.1.11 --duration=30s

# This will:
# 1. Drop packets from Mac #1
# 2. Wait 30 seconds
# 3. Restore network
# 4. Verify consensus recovery
```

### Scenario 5: Load Test (Distributed)

**Objective**: Stress test with distributed load generation

```bash
# Coordinator (Mac M1 #2)
./scripts/distributed-load-test.sh \
    --target-tps=30000 \
    --duration=300s \
    --workers=192.168.1.10,192.168.1.11,192.168.1.12
```

This spawns load generators on all nodes targeting combined 30k TPS.

---

## Monitoring Setup

### Real-Time Dashboard

Access Grafana at `http://192.168.1.12:3000`

Pre-configured dashboards:
1. **Multi-Node Overview**: All nodes health
2. **Treasury Dashboard**: Disbursement processing across executors
3. **Energy Market Dashboard**: Oracle performance
4. **Consensus Dashboard**: Block production metrics
5. **Network Dashboard**: Inter-node latency and bandwidth

### CLI Monitoring

```bash
# Watch cluster status
watch -n 5 '
    echo "Primary (PC):";
    ssh the-block@192.168.1.10 "/usr/local/bin/the-block status";
    echo "";
    echo "Replica 1 (Mac M1):";
    ssh user@192.168.1.11 "/usr/local/bin/the-block status";
    echo "";
    echo "Observer (Mac M1):";
    /usr/local/bin/the-block status
'
```

### Automated Health Checks

Create `/opt/the-block/scripts/cluster-health-check.sh`:

```bash
#!/bin/bash
# Automated cluster health check

NODES=("192.168.1.10" "192.168.1.11" "192.168.1.12")
FAILED=0

for node in "${NODES[@]}"; do
    if ! curl -s "http://$node:9100/metrics" > /dev/null; then
        echo "❌ Node $node is DOWN"
        FAILED=1
    else
        echo "✅ Node $node is UP"
    fi
done

exit $FAILED
```

Run every minute via cron:
```cron
* * * * * /opt/the-block/scripts/cluster-health-check.sh >> /var/log/the-block/health-check.log
```

---

## Troubleshooting

### Node Can't Connect to Peers

```bash
# Check network connectivity
ping -c 3 192.168.1.10
ping -c 3 192.168.1.11

# Check firewall
sudo ufw status  # Ubuntu
pfctl -s rules   # macOS

# Check if node is listening
netstat -tuln | grep 9001

# Check bootstrap config
grep bootstrap_nodes /var/lib/the-block/config/the-block.toml
```

### High Latency Between Nodes

```bash
# Measure latency
ping -c 100 192.168.1.10 | tail -1

# If WiFi is slow, use Ethernet adapters for Mac M1s
# Recommended: USB-C to Gigabit Ethernet adapter

# Check network quality
iperf3 -s  # On one node
iperf3 -c 192.168.1.10  # On another node
```

### Dashboard Not Showing Data

```bash
# Check Prometheus is scraping
curl http://192.168.1.12:9090/api/v1/targets | jq '.data.activeTargets[] | {job, health}'

# Check node metrics endpoint
curl http://192.168.1.10:9100/metrics | head -20

# Restart Grafana
brew services restart grafana
```

### Executor Failover Not Working

```bash
# Check lease TTL
curl http://192.168.1.10:9101/treasury/executor/status | jq '.lease_expires_at'

# Check clocks are synchronized
sudo ntpdate -q pool.ntp.org  # All nodes should be within 1 second

# Force failover test
ssh the-block@192.168.1.10 'systemctl stop the-block-treasury'
sleep 65
curl http://192.168.1.11:9101/treasury/executor/status
```

---

## Appendix

### Cluster Startup Sequence

```bash
# 1. Start primary node first
ssh the-block@192.168.1.10 'systemctl start the-block'

# 2. Wait for primary to initialize (30s)
sleep 30

# 3. Start replica nodes
ssh user@192.168.1.11 'launchctl start com.theblock.node'
launchctl start com.theblock.node  # On Mac #2

# 4. Verify all nodes synced
./scripts/cluster-status.sh
```

### Performance Tuning

**PC (Primary)**:
- Enable huge pages for better memory performance
- Increase file descriptor limits
- Use CPU governor "performance"

**Mac M1**:
- Disable energy saving mode
- Close background applications
- Use cooling pad if running sustained load

### Cost Estimate

- PC: Existing hardware
- Mac M1 Air #1: ~$999
- Mac M1 Air #2: ~$999
- Network switch (if needed): ~$50
- **Total**: ~$2,048 for complete test cluster
