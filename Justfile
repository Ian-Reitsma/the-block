set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @echo "Available recipes: demo"

demo:
    @if [ ! -x .venv/bin/python ]; then \
        echo "virtualenv missing; run ./scripts/bootstrap.sh" >&2; exit 1; \
    fi
    .venv/bin/python demo.py

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
probe:tip:
    cargo run -p probe -- tip --timeout 5

probe:mine:
    cargo run -p probe -- mine-one --timeout 5

probe:gossip:
    cargo run -p probe -- gossip-check --timeout 10

support:bundle:
    bash scripts/support_bundle.sh
