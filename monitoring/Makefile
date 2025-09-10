.PHONY: lint dashboard

dashboard:
	cargo build --manifest-path Cargo.toml >/dev/null
	jq -e . grafana/dashboard.json >/dev/null

lint: dashboard
	npx jsonnet-lint grafana/dashboard.json

