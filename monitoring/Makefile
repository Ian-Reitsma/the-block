.PHONY: lint gen

gen:
	./tools/gen_dashboard.py

lint: gen
	npx jsonnet-lint grafana/dashboard.json
