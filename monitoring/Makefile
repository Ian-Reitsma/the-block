.PHONY: lint dashboard

dashboard:
	cargo build --manifest-path Cargo.toml >/dev/null
	jq -e . grafana/dashboard.json >/dev/null

lint: dashboard
	node tools/jsonnet-lint/index.js grafana/dashboard.json

