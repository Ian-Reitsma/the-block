# Cross-Platform Deployment Guide
> **Review (2025-09-25):** Synced Cross-Platform Deployment Guide guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This guide aggregates the project’s deployment tooling across desktop
operating systems, containerized environments, and infrastructure-as-code
modules. Use it as the canonical reference when packaging or launching nodes
in production.

## Binary Packaging

Prebuilt binaries target Linux (glibc and musl), macOS, and Windows. The build
matrix in `Justfile` invokes `cargo build --release` for each target and emits
artifacts under `target/<triple>/release/`.

* **Linux:** Static musl builds allow deployment on minimal distributions.
* **macOS:** Universal binaries support both Intel and Apple Silicon.
* **Windows:** The PowerShell bootstrap script (`scripts/bootstrap.ps1`)
  installs the required toolchain and sets PATH entries.

Operators can also compile from source using the `bootstrap.sh` script which
installs Rust, Node, and other prerequisites.

## Docker Images

`deploy/docker/Dockerfile` produces multi-architecture images via
`docker buildx build --platform linux/amd64,linux/arm64`. Images publish to the
registry specified by `$TB_DOCKER_REGISTRY` with semantic tags based on the git
commit and crate version.

Example build and push:

```bash
$ docker buildx build -f deploy/docker/Dockerfile \
    --platform linux/amd64,linux/arm64 \
    -t $TB_DOCKER_REGISTRY/the-block:$(git rev-parse --short HEAD) --push .
```

## Terraform Modules

Infrastructure templates live under `deploy/terraform/` with subdirectories for
AWS, GCP, and Azure. Each module provisions compute instances, persistent
volumes, and network security groups.

Usage pattern:

```bash
$ cd deploy/terraform/aws
$ terraform init
$ terraform apply -var="instance_count=3" -var="region=us-west-2"
```

Outputs expose RPC endpoints and health-check URLs for monitoring.

## Ansible Playbooks

Configuration management resides in `deploy/ansible/`. Playbooks install the
node binary, configure systemd services, and manage log rotation.

Key variables:

- `tb_user`: system account running the node
- `tb_data_dir`: location for ledger and snapshots
- `tb_rpc_port`: JSON-RPC bind port

Example execution:

```bash
$ ansible-playbook -i inventory.ini deploy/ansible/node.yml \
    -e tb_user=block -e tb_data_dir=/var/lib/the-block
```

## Local Multi-Service Stacks

Developers can spin up a miniature network using the provided
`deploy/docker-compose.yml`. The compose file orchestrates a validator, gateway,
explorer, and monitoring stack with default ports.

```bash
$ docker compose -f deploy/docker-compose.yml up
```

Services expose metrics on `localhost:9090` and the explorer on
`http://localhost:3000`.

## Additional Notes

- All deployment artifacts are reproducible; `scripts/verify_release.sh`
  checksums Docker images and binaries before publication.
- Terraform and Ansible modules share a common variable schema documented in
  `deploy/README.md`.
- For air‑gapped environments, pre-seed the `bootstrap.sh` caches and copy
  Docker images into a local registry.
