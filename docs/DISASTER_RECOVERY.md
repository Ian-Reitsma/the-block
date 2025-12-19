# Disaster Recovery & Business Continuity Plan

**Last Updated**: 2025-12-19
**Version**: 1.0
**Owner**: SRE Team

---

## Table of Contents

- [Overview](#overview)
- [Recovery Objectives](#recovery-objectives)
- [Backup Strategy](#backup-strategy)
- [Restore Procedures](#restore-procedures)
- [Failover Procedures](#failover-procedures)
- [Test Schedule](#test-schedule)
- [Contact Information](#contact-information)

---

## Overview

This document defines disaster recovery procedures for The Block blockchain infrastructure, covering:

- **Treasury system**: Disbursement records, executor state
- **Energy market**: Oracle data, dispute records
- **Storage market**: Contract metadata, proof records
- **Governance data**: Proposals, voting records, parameter history
- **Blockchain state**: Account balances, transaction history

### Disaster Scenarios

| Scenario | Likelihood | Impact | RTO | RPO |
|----------|-----------|--------|-----|-----|
| Single node failure | High | Low | 5 min | 0 |
| Data center outage | Medium | Medium | 1 hour | 15 min |
| Database corruption | Low | High | 4 hours | 1 hour |
| Complete data loss | Very Low | Critical | 24 hours | 4 hours |
| Malicious attack | Low | High | 4 hours | 1 hour |

---

## Recovery Objectives

### Recovery Time Objective (RTO)

**Target**: System fully operational within specified time after incident

- **Critical services** (block production, treasury executor): 1 hour
- **Non-critical services** (analytics, historical queries): 4 hours
- **Complete rebuild from backups**: 24 hours

### Recovery Point Objective (RPO)

**Target**: Maximum acceptable data loss

- **Blockchain state**: 0 (consensus prevents data loss)
- **Treasury disbursement records**: 15 minutes (backup interval)
- **Market data** (energy, storage): 1 hour (acceptable re-aggregation window)
- **Governance proposals**: 15 minutes (backup interval)

---

## Backup Strategy

### Backup Schedule

| Data Type | Frequency | Retention | Location | Method |
|-----------|-----------|-----------|----------|--------|
| Blockchain state | Every block | 30 days | S3 + Local | Incremental snapshots |
| Treasury data | Every 15 min | 90 days | S3 + Local | Full dump |
| Governance DB | Every 15 min | 90 days | S3 + Local | pg_dump |
| Energy market | Hourly | 30 days | S3 | Aggregated records |
| Storage contracts | Hourly | 180 days | S3 + IPFS | Contract metadata |
| Prometheus metrics | Daily | 90 days | S3 | TSDB snapshots |
| Logs | Continuous | 30 days | S3 + CloudWatch | Streaming |

### Backup Commands

#### Treasury Database Backup

```bash
#!/bin/bash
# /opt/the-block/scripts/backup-treasury.sh

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/var/backups/the-block/treasury"
S3_BUCKET="s3://the-block-backups/treasury"

# Create backup
mkdir -p "$BACKUP_DIR"
the-block treasury export --output "$BACKUP_DIR/treasury_$TIMESTAMP.json"

# Compress
gzip "$BACKUP_DIR/treasury_$TIMESTAMP.json"

# Upload to S3
aws s3 cp "$BACKUP_DIR/treasury_$TIMESTAMP.json.gz" \
  "$S3_BUCKET/treasury_$TIMESTAMP.json.gz" \
  --storage-class STANDARD_IA

# Keep local copies for 7 days
find "$BACKUP_DIR" -name "treasury_*.json.gz" -mtime +7 -delete

echo "Treasury backup completed: $TIMESTAMP"
```

#### Governance Database Backup

```bash
#!/bin/bash
# /opt/the-block/scripts/backup-governance.sh

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/var/backups/the-block/governance"
S3_BUCKET="s3://the-block-backups/governance"

mkdir -p "$BACKUP_DIR"

# PostgreSQL dump (if using Postgres)
# pg_dump -h localhost -U governance -Fc governance_db \
#   > "$BACKUP_DIR/governance_$TIMESTAMP.dump"

# Or native export
the-block governance export --output "$BACKUP_DIR/governance_$TIMESTAMP.json"

gzip "$BACKUP_DIR/governance_$TIMESTAMP.json"

aws s3 cp "$BACKUP_DIR/governance_$TIMESTAMP.json.gz" \
  "$S3_BUCKET/governance_$TIMESTAMP.json.gz"

find "$BACKUP_DIR" -name "governance_*.json.gz" -mtime +7 -delete

echo "Governance backup completed: $TIMESTAMP"
```

#### Blockchain State Snapshot

```bash
#!/bin/bash
# /opt/the-block/scripts/backup-blockchain.sh

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/var/backups/the-block/blockchain"
S3_BUCKET="s3://the-block-backups/blockchain"
DATA_DIR="/var/lib/the-block/data"

mkdir -p "$BACKUP_DIR"

# Create snapshot
the-block snapshot create \
  --data-dir "$DATA_DIR" \
  --output "$BACKUP_DIR/snapshot_$TIMESTAMP.tar.zst"

# Upload to S3
aws s3 cp "$BACKUP_DIR/snapshot_$TIMESTAMP.tar.zst" \
  "$S3_BUCKET/snapshot_$TIMESTAMP.tar.zst" \
  --storage-class GLACIER_IR

# Keep only latest 3 local snapshots
ls -t "$BACKUP_DIR"/snapshot_*.tar.zst | tail -n +4 | xargs -r rm

echo "Blockchain snapshot completed: $TIMESTAMP"
```

### Automated Backup via Cron

```cron
# /etc/cron.d/the-block-backups

# Treasury backup every 15 minutes
*/15 * * * * backup /opt/the-block/scripts/backup-treasury.sh >> /var/log/the-block/backup-treasury.log 2>&1

# Governance backup every 15 minutes
*/15 * * * * backup /opt/the-block/scripts/backup-governance.sh >> /var/log/the-block/backup-governance.log 2>&1

# Blockchain snapshot every 6 hours
0 */6 * * * backup /opt/the-block/scripts/backup-blockchain.sh >> /var/log/the-block/backup-blockchain.log 2>&1

# Prometheus snapshot daily at 2 AM
0 2 * * * backup /opt/the-block/scripts/backup-prometheus.sh >> /var/log/the-block/backup-prometheus.log 2>&1
```

---

## Restore Procedures

### Treasury Data Restore

**Scenario**: Treasury database corrupted or lost

**Prerequisites**:
- Access to S3 backups
- The Block node stopped
- Backup timestamp identified

**Steps**:

```bash
# 1. Stop treasury executor
systemctl stop the-block-treasury

# 2. Download latest backup
BACKUP_FILE=$(aws s3 ls s3://the-block-backups/treasury/ | sort | tail -n 1 | awk '{print $4}')
aws s3 cp "s3://the-block-backups/treasury/$BACKUP_FILE" /tmp/

# 3. Decompress
gunzip "/tmp/$BACKUP_FILE"

# 4. Restore
the-block treasury import --input "/tmp/${BACKUP_FILE%.gz}" --force

# 5. Verify integrity
the-block treasury verify --all

# 6. Restart executor
systemctl start the-block-treasury

# 7. Monitor logs
journalctl -u the-block-treasury -f
```

**Expected Duration**: 15-30 minutes
**Validation**: Check disbursement count matches expected value

### Governance Data Restore

```bash
# 1. Stop node
systemctl stop the-block

# 2. Download backup
BACKUP_FILE=$(aws s3 ls s3://the-block-backups/governance/ | sort | tail -n 1 | awk '{print $4}')
aws s3 cp "s3://the-block-backups/governance/$BACKUP_FILE" /tmp/
gunzip "/tmp/$BACKUP_FILE"

# 3. Restore
the-block governance import --input "/tmp/${BACKUP_FILE%.gz}" --force

# 4. Verify
the-block governance verify --proposals --params

# 5. Restart
systemctl start the-block
```

**Expected Duration**: 10-20 minutes

### Blockchain State Restore

```bash
# 1. Stop node completely
systemctl stop the-block

# 2. Backup current state (safety)
mv /var/lib/the-block/data /var/lib/the-block/data.backup

# 3. Download snapshot
SNAPSHOT=$(aws s3 ls s3://the-block-backups/blockchain/ | grep snapshot | sort | tail -n 1 | awk '{print $4}')
aws s3 cp "s3://the-block-backups/blockchain/$SNAPSHOT" /tmp/

# 4. Extract snapshot
mkdir -p /var/lib/the-block/data
tar -xf "/tmp/$SNAPSHOT" -C /var/lib/the-block/data

# 5. Set permissions
chown -R the-block:the-block /var/lib/the-block/data

# 6. Start node
systemctl start the-block

# 7. Monitor sync (will sync remaining blocks from network)
the-block status --watch
```

**Expected Duration**: 2-4 hours (depending on snapshot age)

### Complete System Rebuild

**Scenario**: Total infrastructure loss, rebuilding from zero

**Prerequisites**:
- New infrastructure provisioned
- DNS updated to new IPs
- All backup credentials available

**Steps**:

```bash
# 1. Provision infrastructure (Terraform/manual)
terraform apply -var-file=disaster-recovery.tfvars

# 2. Install The Block
curl -sSL https://install.theblock.example.com/latest.sh | bash

# 3. Configure node
cp /mnt/backups/config/the-block.toml /etc/the-block/

# 4. Restore blockchain state (latest snapshot)
./restore-blockchain.sh

# 5. Restore governance data
./restore-governance.sh

# 6. Restore treasury data
./restore-treasury.sh

# 7. Start services
systemctl enable --now the-block
systemctl enable --now the-block-treasury
systemctl enable --now prometheus
systemctl enable --now grafana

# 8. Verify services
./verify-health.sh

# 9. Re-join network
the-block network join --bootstrap nodes.theblock.example.com

# 10. Monitor sync progress
watch -n 5 the-block status
```

**Expected Duration**: 8-24 hours (depending on blockchain size)

---

## Failover Procedures

### Multi-Node Failover

The Block supports active-active replication across multiple nodes.

**Topology**:
```
Primary Node (PC)    → Replica 1 (Mac M1 #1) → Replica 2 (Mac M1 #2)
      |                       |                       |
   Primary            Hot Standby             Warm Standby
   Treasury             Read-Only              Read-Only
```

**Automatic Failover**:

```bash
# Configure failover in the-block.toml

[failover]
enabled = true
health_check_interval = 10s
failover_timeout = 30s

[[failover.nodes]]
address = "192.168.1.10:9001"  # PC primary
priority = 100

[[failover.nodes]]
address = "192.168.1.11:9001"  # Mac M1 #1
priority = 90

[[failover.nodes]]
address = "192.168.1.12:9001"  # Mac M1 #2
priority = 80
```

**Manual Failover**:

```bash
# On current primary
the-block treasury executor stop

# On new primary (Mac M1 #1)
the-block treasury executor promote --force

# Verify
the-block treasury executor status
```

### Database Failover

If using PostgreSQL for governance data:

```bash
# Promote standby to primary
pg_ctl promote -D /var/lib/postgresql/14/governance

# Update application config
sed -i 's/postgres-primary/postgres-standby1/' /etc/the-block/the-block.toml

# Restart services
systemctl restart the-block
```

---

## Test Schedule

### Quarterly Disaster Recovery Drills

**Q1 (March)**: Treasury data restore test
**Q2 (June)**: Complete system rebuild from backups
**Q3 (September)**: Multi-node failover test
**Q4 (December)**: Simulated data center outage

### Monthly Backup Validation

- **Week 1**: Verify backup integrity (checksums, test restore to staging)
- **Week 2**: Test restore procedures (non-critical data)
- **Week 3**: Failover drill (treasury executor)
- **Week 4**: Review and update runbooks

---

## Monitoring & Alerting

### Backup Health Metrics

Prometheus metrics to monitor:

- `backup_last_success_timestamp_seconds{job="backup-treasury"}`
- `backup_last_success_timestamp_seconds{job="backup-governance"}`
- `backup_last_success_timestamp_seconds{job="backup-blockchain"}`
- `backup_size_bytes{job="backup-*"}`
- `backup_duration_seconds{job="backup-*"}`

### Alerts

```yaml
# Alert if backup hasn't succeeded in 2 hours
- alert: BackupStale
  expr: time() - backup_last_success_timestamp_seconds > 7200
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Backup {{ $labels.job }} hasn't succeeded in 2+ hours"
```

---

## Contact Information

### Escalation Matrix

| Role | Name | Phone | Email | Availability |
|------|------|-------|-------|--------------|
| SRE On-Call | Rotating | +1-555-SRE-ONCALL | oncall@theblock.example.com | 24/7 |
| Treasury Lead | TBD | +1-555-TREASURY | treasury-lead@theblock.example.com | Business hours |
| Infrastructure Lead | TBD | +1-555-INFRA | infra-lead@theblock.example.com | Business hours |
| Security Officer | TBD | +1-555-SECURITY | security@theblock.example.com | 24/7 |

### External Vendors

- **AWS Support**: Enterprise support plan, case priority: Urgent
- **Database Consulting**: contact@db-consulting.example.com
- **Security Incident Response**: security-ir@theblock.example.com

---

## Appendix

### Backup Retention Policy

- **Hot backups** (S3 Standard): 7 days
- **Warm backups** (S3 Standard-IA): 30 days
- **Cold backups** (S3 Glacier): 90 days
- **Archive** (S3 Glacier Deep Archive): 1 year

### Compliance

- **SOC 2 Type II**: Annual audit required
- **GDPR**: 30-day data retention after deletion request
- **Internal Policy**: 90-day minimum retention for financial records (treasury)

### Change Log

| Date | Version | Author | Changes |
|------|---------|--------|---------|
| 2025-12-19 | 1.0 | SRE Team | Initial DR plan |
