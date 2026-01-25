use http_env::blocking_client as env_blocking_client;
use httpd::BlockingClient;

const DEFAULT_PREFIXES: &[&str] = &["TB_RPC_TLS", "TB_HTTP_TLS"];

pub fn blocking_client() -> BlockingClient {
    // Check if plain HTTP mode is requested (for testing with mock servers)
    let plain_mode = std::env::var("TB_HTTP_PLAIN").is_ok();
    eprintln!("[HTTP_CLIENT] TB_HTTP_PLAIN env var check: {}", plain_mode);
    eprintln!("[HTTP_CLIENT] Checking TLS env vars...");

    // Check what TLS env vars are set
    for prefix in DEFAULT_PREFIXES {
        for suffix in &["_CA_CERT", "_CLIENT_CERT", "_CLIENT_KEY"] {
            let var_name = format!("{}{}", prefix, suffix);
            if let Ok(value) = std::env::var(&var_name) {
                eprintln!("[HTTP_CLIENT] {} = {} (FOUND)", var_name, value);
            } else {
                eprintln!("[HTTP_CLIENT] {} = <not set>", var_name);
            }
        }
    }

    if plain_mode {
        eprintln!("[HTTP_CLIENT] TB_HTTP_PLAIN is set, using plain HTTP client (no TLS)");
        let config = httpd::ClientConfig {
            connect_timeout: std::time::Duration::from_secs(5),
            request_timeout: std::time::Duration::from_secs(15),
            read_timeout: Some(std::time::Duration::from_secs(15)),
            tls_handshake_timeout: std::time::Duration::from_secs(10),
            max_response_bytes: 16 * 1024 * 1024,
            tls: None, // Explicitly disable TLS
        };
        return BlockingClient::new(config);
    }
    eprintln!("[HTTP_CLIENT] TB_HTTP_PLAIN not set, using env_blocking_client");
    env_blocking_client(DEFAULT_PREFIXES, "cli")
}
