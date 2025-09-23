.PHONY: monitor fuzz-wal --native-monitor dependency-check
monitor:
	@if [ "$(filter --native-monitor,$(MAKECMDGOALS))" != "" ] || ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then \
	bash scripts/monitor_native.sh; \
	else \
	docker compose -f monitoring/docker-compose.yml up; \
	fi

--native-monitor:
	@:

fuzz-wal:
	cargo +nightly fuzz run wal_fuzz -- -max_total_time=60 -artifact_prefix=fuzz/wal/ -runs=0

dashboard:
	make -C monitoring dashboard

doc-ci:
        @rustc tools/refcheck.rs -O -o /tmp/refcheck
        @/tmp/refcheck .
        cargo test --doc --all-features

dependency-check:
	cargo run -p dependency_registry -- --check config/dependency_policies.toml
