# httpd crate

The `httpd` crate provides the in-house HTTP server, client, and JSON-RPC
plumbing that replaces the previous third-party stacks described in
[`docs/pivot_dependency_strategy.md`](../../docs/pivot_dependency_strategy.md)
and [`docs/progress.md`](../../docs/progress.md). Consumers in the workspace
should route new services through this crate instead of introducing external
web frameworks.

## Serving HTTP and HTTPS

Use [`serve`](src/lib.rs) to bind plain HTTP listeners and
[`serve_tls`](src/lib.rs) when TLS is required. The TLS helper is backed by the
first-party implementation under [`tls.rs`](src/tls.rs), loading Ed25519
certificates, performing X25519 handshakes, and sealing records with the in-house
AEAD pipeline. [`ServerTlsConfig`] exposes convenience constructors for plain or
mutual-authentication deployments so callers can swap existing services onto the
custom stack without rewriting their routing logic.

```rust
use httpd::{serve_tls, Response, Router, ServerConfig, ServerTlsConfig, StatusCode};
use runtime::net::TcpListener;

# async fn start() -> std::io::Result<()> {
let listener = TcpListener::bind("127.0.0.1:8443".parse().unwrap()).await?;
let router = Router::new(())
    .get("/healthz", |_req| async move {
        Ok(
            Response::new(StatusCode::OK)
                .json(&foundation_serialization::json!({ "status": "ok" }))?,
        )
    });
let tls = ServerTlsConfig::from_pem_files("certs/server.pem", "certs/server-key.pem")?;
serve_tls(listener, router, ServerConfig::default(), tls).await?;
# Ok(())
# }
```

## Testing handlers without sockets

[`Router::handle`] accepts a pre-built [`Request`] so tests can exercise
handlers directly. [`RequestBuilder`] wraps the internal request structure with
simple setters for HTTP method, path, query parameters, headers, and bodies,
and automatically supplies the `Host` header expected by production traffic.

```rust
use httpd::{RequestBuilder, Response, Router, StatusCode};
use runtime::block_on;

let router = Router::new(())
    .get("/ping", |_req| async move {
        Ok(Response::new(StatusCode::OK).with_body(b"pong".to_vec()))
    });
let response = block_on(async {
    let request = router
        .request_builder()
        .path("/ping")
        .query_param("echo", "true")
        .build();
    router.handle(request).await
}).unwrap();
assert_eq!(response.status(), StatusCode::OK);
```

Downstream migrations should target these interfaces so that Axum/Hyper can be
removed incrementally across the node, metrics aggregator, and gateway.
