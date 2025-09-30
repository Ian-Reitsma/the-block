use crate::codec_helpers::json_from_str;
use clap::Subcommand;
use light_client::{load_user_config, LightClientConfig, StateChunk, StateStream};
use runtime::net::TcpStream;
use runtime::ws::{self, ClientStream, Message as WsMessage};
use url::Url;

#[derive(Subcommand)]
pub enum LightSyncCmd {
    /// Start light-client synchronization over a websocket URL
    Start { url: String },
}

pub fn handle(cmd: LightSyncCmd) {
    match cmd {
        LightSyncCmd::Start { url } => {
            runtime::block_on(async move {
                match connect_state_ws(&url).await {
                    Ok(mut ws) => {
                        let config: LightClientConfig = load_user_config().unwrap_or_default();
                        let mut stream = StateStream::from_config(&config);
                        let _ = ws.send(WsMessage::Ping(Vec::new())).await;
                        while let Ok(Some(msg)) = ws.recv().await {
                            match msg {
                                WsMessage::Text(text) => {
                                    if let Ok(chunk) = json_from_str::<StateChunk>(&text) {
                                        if let Err(err) = stream.apply_chunk(chunk.clone()) {
                                            eprintln!("failed to apply chunk: {err}");
                                        }
                                        if stream.lagging(chunk.tip_height) {
                                            #[cfg(feature = "telemetry")]
                                            the_block::telemetry::STATE_STREAM_LAG_ALERT_TOTAL
                                                .inc();
                                        }
                                    }
                                }
                                WsMessage::Binary(bytes) => {
                                    if let Ok(text) = String::from_utf8(bytes) {
                                        if let Ok(chunk) = json_from_str::<StateChunk>(&text) {
                                            if let Err(err) = stream.apply_chunk(chunk.clone()) {
                                                eprintln!("failed to apply chunk: {err}");
                                            }
                                            if stream.lagging(chunk.tip_height) {
                                                #[cfg(feature = "telemetry")]
                                                the_block::telemetry::STATE_STREAM_LAG_ALERT_TOTAL
                                                    .inc();
                                            }
                                        }
                                    } else {
                                        eprintln!("ignored non-utf8 state chunk frame");
                                    }
                                }
                                WsMessage::Close(_) => break,
                                WsMessage::Ping(payload) => {
                                    let _ = ws.send(WsMessage::Pong(payload)).await;
                                }
                                WsMessage::Pong(_) => {}
                            }
                        }
                    }
                    Err(e) => eprintln!("{}", e),
                }
            });
        }
    }
}

async fn connect_state_ws(url: &str) -> Result<ClientStream, String> {
    let parsed = Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "ws" {
        return Err(format!("unsupported scheme {}", parsed.scheme()));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "missing host in websocket url".to_string())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| e.to_string())?;
    let addr = addrs
        .next()
        .ok_or_else(|| "no addresses resolved for websocket host".to_string())?;
    let mut stream = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;

    let key = ws::handshake_key();
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    let default_port = 80;
    let host_header = if port == default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let expected_accept = ws::handshake_accept(&key);
    ws::read_client_handshake(&mut stream, &expected_accept)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ClientStream::new(stream))
}
