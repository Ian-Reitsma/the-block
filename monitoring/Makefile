.PHONY: lint ../node_modules

lint: ../node_modules
		@npx jsonnet-lint grafana/dashboard.json

../node_modules:
		@cd .. && npm ci
