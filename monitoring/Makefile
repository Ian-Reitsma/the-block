.PHONY: lint gen schema

schema:
	./tools/gen_schema.py

gen: schema
	./tools/gen_dashboard.py

lint: gen
	npx jsonnet-lint grafana/dashboard.json
