use clap::Subcommand;
use futures::{SinkExt, StreamExt};
use light_client::{load_user_config, LightClientConfig, StateChunk, StateStream};

#[derive(Subcommand)]
pub enum LightSyncCmd {
    /// Start light-client synchronization over a websocket URL
    Start { url: String },
}

pub fn handle(cmd: LightSyncCmd) {
    match cmd {
        LightSyncCmd::Start { url } => {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            rt.block_on(async move {
                match tokio_tungstenite::connect_async(url).await {
                    Ok((ws, _)) => {
                        let (mut write, mut read) = ws.split();
                        let config: LightClientConfig = load_user_config().unwrap_or_default();
                        let mut stream = StateStream::from_config(&config);
                        let _ = write
                            .send(tokio_tungstenite::tungstenite::Message::Ping(vec![]))
                            .await;
                        while let Some(Ok(msg)) = read.next().await {
                            if msg.is_text() {
                                if let Ok(chunk) =
                                    serde_json::from_str::<StateChunk>(msg.to_text().unwrap())
                                {
                                    if let Err(err) = stream.apply_chunk(chunk.clone()) {
                                        eprintln!("failed to apply chunk: {err}");
                                    }
                                    if stream.lagging(chunk.tip_height) {
                                        #[cfg(feature = "telemetry")]
                                        the_block::telemetry::STATE_STREAM_LAG_ALERT_TOTAL.inc();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("{}", e),
                }
            });
        }
    }
}
