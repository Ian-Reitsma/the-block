#![forbid(unsafe_code)]

use concurrency::Lazy;
use energy_market::UnixTimestamp;
use foundation_serialization::Serialize;
use httpd::{serve, HttpError, Request, Response, Router, ServerConfig, StatusCode};
use oracle_adapter::MeterReadingPayload;
use runtime::block_on;
use runtime::net::TcpListener;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static START_TS: Lazy<UnixTimestamp> = Lazy::new(|| {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
});

#[derive(Default)]
struct MeterState {
    total_kwh: u64,
    last_timestamp: UnixTimestamp,
}

#[derive(Default)]
struct AppState {
    meters: Mutex<HashMap<String, MeterState>>,
}

impl AppState {
    fn next_reading(&self, meter_id: &str) -> MeterReadingPayload {
        let mut guard = self.meters.lock().expect("meter lock");
        let meter = guard.entry(meter_id.to_string()).or_default();
        meter.total_kwh = meter.total_kwh.saturating_add(250);
        meter.last_timestamp = current_timestamp();
        MeterReadingPayload::new(
            format!("provider-{meter_id}"),
            meter_id.to_string(),
            meter.total_kwh,
            meter.last_timestamp,
            mock_signature(meter_id, meter.total_kwh),
        )
    }

    fn accept_reading(&self, meter_id: &str, reading: &MeterReadingPayload) {
        let mut guard = self.meters.lock().expect("meter lock");
        let entry = guard.entry(meter_id.to_string()).or_default();
        entry.total_kwh = entry.total_kwh.max(reading.kwh_reading);
        entry.last_timestamp = reading.timestamp.max(entry.last_timestamp);
    }
}

#[derive(Serialize)]
#[serde(crate = "foundation_serialization::serde")]
struct SubmitAck<'a> {
    status: &'a str,
}

fn current_timestamp() -> UnixTimestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(*START_TS)
}

fn mock_signature(id: &str, value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(id.as_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

async fn handle_get(req: Request<AppState>) -> Result<Response, HttpError> {
    let meter_id = req.param("id").unwrap_or("default");
    let reading = req.state().next_reading(meter_id);
    Response::new(StatusCode::OK).json(&reading)
}

async fn handle_submit(req: Request<AppState>) -> Result<Response, HttpError> {
    let meter_id = req.param("id").unwrap_or("default");
    let reading: MeterReadingPayload = req.json()?;
    req.state().accept_reading(meter_id, &reading);
    Response::new(StatusCode::OK).json(&SubmitAck { status: "ok" })
}

fn main() -> io::Result<()> {
    block_on(async {
        let addr = std::env::var("MOCK_ENERGY_ORACLE_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string());
        let socket: SocketAddr = addr
            .parse()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        let listener = TcpListener::bind(socket).await?;
        println!("mock-energy-oracle listening on {socket}");
        let router = Router::new(AppState::default())
            .get("/meter/:id/reading", handle_get)
            .post("/meter/:id/submit", handle_submit);
        serve(listener, router, ServerConfig::default()).await
    })
}
