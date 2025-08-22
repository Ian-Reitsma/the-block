.PHONY: monitor fuzz-wal --native-monitor
monitor:
	@if [ "$(filter --native-monitor,$(MAKECMDGOALS))" != "" ] || ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then \
	bash scripts/monitor_native.sh; \
	else \
	docker compose -f monitoring/docker-compose.yml up; \
	fi

--native-monitor:
	@:

fuzz-wal:
	cargo fuzz run wal_fuzz --max-total-time=60 -- -artifact_prefix=fuzz/wal/
