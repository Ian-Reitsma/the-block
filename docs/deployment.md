# Deployment Guide
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document covers high-availability setups for The-Block nodes and gateways.

## High Availability
- Run multiple nodes behind a load balancer.
- Use the provided Helm charts under `deploy/helm` for Kubernetes deployments with probes.

## Firewall Rules
- Open TCP ports `8001-8002` for P2P and `8545-8546` for RPC when using Docker Compose.
- Allow Grafana on port `3000` and explorer on port `8080` for monitoring.

## Monitoring
- Grafana dashboards are exposed via the Docker Compose stack.
- Metrics can be shipped to an S3-compatible store using the metrics-aggregator `s3` feature.