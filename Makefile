.PHONY: monitor fuzz-wal
monitor:
	docker compose -f monitoring/docker-compose.yml up

fuzz-wal:
	cargo fuzz run wal_fuzz --max-iters=1000
