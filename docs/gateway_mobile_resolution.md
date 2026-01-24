# .block Mobile Resolver Runbook

## Overview
Operators expose `.block` domains through the gateway binary (`node/src/bin/gateway.rs`) so that browsers, mobile apps, and DoH-enabled DNS clients can fetch `https://<domain>/` content and resolve `.block` names via `/dns/resolve`. This runbook describes how to wire the binary, TLS artifacts, stake gating, and resolver knobs together, then configures phones (Android/iOS/desktop DoH) to browse `.block` reliably.

## 1. Gateway daemon + systemd
1. Build the gateway daemon by enabling the CLI and gateway features of `the_block`.
   ```bash
   cargo build -p the_block --bin gateway --locked --release --features "cli gateway"
   ```
   The resulting binary is `target/release/gateway`.
2. Install or copy the binary into `/usr/local/bin/gateway` and ensure it is executable.
3. The service at `deploy/systemd/gateway.service` stays unchanged: it stages TLS via `contract tls stage [...]` and executes `/usr/local/bin/gateway --listen 0.0.0.0:9000`. Adjust `--listen` or `--config-dir` only when customizing the cluster.
4. The gateway CLI accepts TLS overrides via `--tls-cert`, `--tls-key`, `--tls-client-ca`, and `--tls-client-ca-optional`. Matching env vars (`TB_GATEWAY_TLS_*`) follow the `http_env` naming pattern for automated deployments.
5. Board stake gating before the binary starts: the HTTP stack rejects requests whose `Host` header does not map to a domain with a funded entry in `dns_ownership/<domain>` (see `node/src/gateway/dns.rs`). Use the DNS auction/stake CLI to mint the domain and deposit BLOCK before adding it to the resolver list.

## 2. Resolver knobs (for phones)
- `TB_GATEWAY_RESOLVER_ADDRS`: comma-separated IPv4/IPv6 addresses advertised for `.block` DoH answers. Populate this with the gateway server's reachable IPs so clients can connect. Leave empty when you emit a CNAME instead.
- `TB_GATEWAY_RESOLVER_TTL`: TTL in seconds (default `60`). The gateway writes this value into the JSON `Answer` entries and the `Cache-Control` header so phones honor the desired cache duration.
- `TB_GATEWAY_RESOLVER_CNAME`: optional CNAME target emitted when `TB_GATEWAY_RESOLVER_ADDRS` is empty. Point it at `gateway.example.block` (or any resolvable host) that loops back into your mesh.

The resolver reuses the stake table that guards static assets. If `domain_has_stake(question.name)` returns `false`, DNS queries return HTTP `403` with `Status=3`. A lack of answers despite stake results in HTTP `404` plus `Status=3` so clients know the name exists but no records are configured.

## 3. Phone configuration (DoH)
1. **Android** – open **Settings → Network & internet → Private DNS** and choose `Private DNS provider hostname`. Enter `gateway.example.block` (or whatever host is backed by your TLS cert) and tap Save. Android then sends DNS-over-TLS traffic to that host, which proxies the `/dns/resolve` endpoint.
2. **iOS / macOS** – go to **Settings → Wi-Fi → (i)** next to your network, tap **Configure DNS**, choose **Manual**, add the gateway DoH endpoint under `Add Server` by specifying a profile that supports DoH (e.g., NextDNS, 1.1.1.1). Alternatively, install a custom DoH profile pointing to `https://gateway.example.block/dns/resolve?name=%s&type=%t` so Safari/Apps use the gateway endpoint directly.
3. **Desktop browsers** – Chrome and Firefox have DoH settings in their security/privacy sections. Set the template to `https://gateway.example.block/dns/resolve?name=%s&type=%t` (use the TLS hostname you provisioned). The gateway responds with `content-type: application/dns-json`, `Status=0`, and TTL headers.
4. **Fallback** – scripts and apps that only allow raw DNS can still reach `gateway.example.block` via a trusted DNS stub that resolves the gateway’s `gateway.example.block` host to one of the `TB_GATEWAY_RESOLVER_ADDRS` IP addresses.
5. Always verify the resolver by running `curl -v https://gateway.example.block/dns/resolve?name=foo.block&type=A`. Expect `Status=0` and a TTL-matching `cache-control` header once the domain is funded. Remove the stake and repeat to see `Status=3` plus HTTP `403` as a guardrail test.

## 4. Verification and failover
- Use `TB_GATEWAY_URL` to publish shareable links (`https://<gateway>/drive/<object_id>`). The same JDBC host backs the DoH endpoint, so share this URL inside TXT records or release notes.
- Monitor `node/src/telemetry.rs` counters `GATEWAY_DOH_STATUS_TOTAL` (labels match the `Status` value) and `aggregator_doh_resolver_failure_total` from `/wrappers`. Raise alerts in `monitoring/alert.rules.yml` to catch repeated `Status=3` answers.
- If your gateway front-end sits behind a load balancer, make sure the resolver address list includes every front-end IP, or use a CNAME that points back to the TLS host.

## 5. Ops tips for phone troubleshooting
- A 403 from the DoH endpoint indicates `domain_has_stake` returned `false`. Fund the stake via `contract-cli dns register-stake` or the CLI described in `docs/system_reference.md#9-gateway-dns-and-read-receipts`.
- A `Status=3` response with HTTP `404` means no resolver addresses or CNAME were configured. Set `TB_GATEWAY_RESOLVER_ADDRS`/`TB_GATEWAY_RESOLVER_CNAME` and reload TLS envs via the service’s `ExecReload` hook.
- Use `TB_GATEWAY_RESOLVER_TTL` to tune how often phones re-query the resolver; snapping it to 60 keeps resolver drifts short while still caching the gateway IPs.
- Document any VPN/app-level profiles (iOS/macOS DoH or Android Private DNS) inside `docs/gateway_mobile_resolution.md` so field teams can hand them to testers and perform `curl`/`telnet` pre-flight checks.

## References
- `node/src/bin/gateway.rs` – CLI wiring for TLS, stake, and resolver flags.
- `node/src/web/gateway.rs` – DoH endpoint, stake gating, and static/drive handlers.
- `deploy/systemd/gateway.service` – systemd unit that stages TLS artifacts and runs the binary.
- `docs/operations.md#gateway-service-runbook` – deployment checklist for gateways and the DoH smoke test.
- `docs/system_reference.md#9-gateway-dns-and-read-receipts` – DNS TXT schema, CLI flows, and mobile cache notes.
