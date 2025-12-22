set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @echo "Available recipes: demo docs test-peer-stats"

demo:
    @if [ ! -x .venv/bin/python ]; then \
        echo "virtualenv missing; run ./scripts/bootstrap.sh" >&2; exit 1; \
    fi
    .venv/bin/python demo.py

test-workloads:
    for f in examples/workloads/*.json; do \
        cargo run --example run_workload $$f >/dev/null; \
    done

test-gossip:
    if command -v cargo-nextest >/dev/null 2>&1; then \
        RUST_LOG=info cargo nextest run --all-features gossip_converges_to_longest_chain; \
    else \
        RUST_LOG=info cargo test --all-features gossip_converges_to_longest_chain; \
    fi

swarm-up:
    sh scripts/swarm.sh up

swarm-down:
    sh scripts/swarm.sh down

test-peer-stats:
    cargo test -p the_block --test rate_limit --features telemetry --release -- -q
    cargo test -p the_block --test net_peer_stats --features telemetry --release -- -q

test-range-boost:
    cargo test -p the_block --test range_boost --features "integration-tests telemetry" --release -- -q

test-gateway:
    cargo test -p the_block --lib --features gateway web::gateway::tests::

swarm-logs:
    sh scripts/swarm.sh logs

swarm-test:
    sh scripts/swarm.sh up
    if command -v cargo-nextest >/dev/null 2>&1; then \
        cargo nextest run --all-features --tests net_gossip; \
    else \
        cargo test --all-features --tests net_gossip; \
    fi
    sh scripts/swarm.sh down
probe-tip:
    cargo run -p probe -- tip --timeout 5

probe-mine:
    cargo run -p probe -- mine-one --timeout 5

probe-gossip:
    cargo run -p probe -- gossip-check --timeout 10

support-bundle:
    bash scripts/support_bundle.sh

fuzz-promote:
    bash scripts/promote_wal_seeds.sh

docs:
    mdbook build docs

format:
    cargo fmt --all

lint:
    cargo clippy --all-targets --all-features -- -D warnings

dependency-audit:
    cargo run -p dependency_registry -- --check config/dependency_policies.toml

check-windows:
    rustup target add x86_64-pc-windows-gnu
    FIRST_PARTY_ONLY=1 cargo check --target x86_64-pc-windows-gnu -p sys -p runtime

chaos-suite:
    mkdir -p target/chaos
    TB_CHAOS_ATTESTATIONS=target/chaos/attestations.json \
    TB_CHAOS_STATUS_SNAPSHOT=target/chaos/status.snapshot.json \
    TB_CHAOS_STATUS_DIFF=target/chaos/status.diff.json \
    TB_CHAOS_OVERLAY_READINESS=target/chaos/overlay.readiness.json \
    TB_CHAOS_PROVIDER_FAILOVER=target/chaos/provider.failover.json \
        cargo run -p tb-sim --bin chaos_lab --quiet
