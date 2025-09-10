use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use once_cell::sync::Lazy;
use prometheus::{IntCounter, IntGauge, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerStat {
    pub peer_id: String,
    pub metrics: serde_json::Value,
}

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<Mutex<HashMap<String, VecDeque<(u64, serde_json::Value)>>>>,
    pub token: String,
    path: PathBuf,
    retention_secs: u64,
}

impl AppState {
    pub fn new(token: String, path: impl AsRef<Path>, retention_secs: u64) -> Self {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            data: Arc::new(Mutex::new(data)),
            token,
            path,
            retention_secs,
        }
    }

    fn persist(&self) {
        if let Ok(map) = self.data.lock() {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.path, serde_json::to_vec(&*map).unwrap());
        }
    }

    fn prune(&self) {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(self.retention_secs);
        if let Ok(mut map) = self.data.lock() {
            map.retain(|_, deque| {
                deque.retain(|(ts, _)| *ts >= cutoff);
                !deque.is_empty()
            });
        }
    }
}

static INGEST_TOTAL: Lazy<IntCounter> =
    Lazy::new(|| IntCounter::new("aggregator_ingest_total", "Total peer metric ingests").unwrap());

static ACTIVE_PEERS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "cluster_peer_active_total",
        "Unique peers tracked by aggregator",
    )
    .unwrap()
});

static REGISTRY: Lazy<Registry> = Lazy::new(|| {
    let r = Registry::new();
    r.register(Box::new(INGEST_TOTAL.clone())).unwrap();
    r.register(Box::new(ACTIVE_PEERS.clone())).unwrap();
    r
});

fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
    use serde_json::{Map, Value};
    match b {
        Value::Object(bm) => {
            if !a.is_object() {
                *a = Value::Object(Map::new());
            }
            let am = a.as_object_mut().unwrap();
            for (k, bv) in bm {
                merge(am.entry(k.clone()).or_insert(Value::Null), bv);
            }
        }
        Value::Number(bn) => {
            let sum = a.as_f64().unwrap_or(0.0) + bn.as_f64().unwrap_or(0.0);
            *a = Value::from(sum);
        }
        _ => {
            *a = b.clone();
        }
    }
}

async fn ingest(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<Vec<PeerStat>>,
) -> StatusCode {
    if headers
        .get("x-auth-token")
        .and_then(|h| h.to_str().ok())
        .map(|h| h == state.token)
        .unwrap_or(false)
    {
        let mut map = state.data.lock().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for stat in payload {
            let entry = map.entry(stat.peer_id).or_insert_with(VecDeque::new);
            if let Some((ts, last)) = entry.back_mut() {
                if *ts == now {
                    merge(last, &stat.metrics);
                    continue;
                }
            }
            entry.push_back((now, stat.metrics));
            if entry.len() > 1024 {
                entry.pop_front();
            }
        }
        ACTIVE_PEERS.set(map.len() as i64);
        INGEST_TOTAL.inc();
        drop(map);
        state.prune();
        state.persist();
        StatusCode::OK
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn peer(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
) -> Json<Vec<(u64, serde_json::Value)>> {
    let map = state.data.lock().unwrap();
    let data = map
        .get(&id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    Json(data)
}

async fn cluster(State(state): State<AppState>) -> Json<usize> {
    let map = state.data.lock().unwrap();
    Json(map.len())
}

async fn metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = String::new();
    encoder.encode_utf8(&metric_families, &mut buffer).unwrap();
    buffer
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ingest", post(ingest))
        .route("/peer/:id", get(peer))
        .route("/cluster", get(cluster))
        .route("/metrics", get(metrics))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{self, Body};
    use axum::http::{Request, StatusCode};
    use tempfile::tempdir;
    use tower::ServiceExt; // for `oneshot`

    #[tokio::test]
    async fn dedupes_by_peer() {
        let dir = tempdir().unwrap();
        let state = AppState::new("token".into(), dir.path().join("m.json"), 60);
        let app = router(state.clone());
        let payload = serde_json::json!([
            {"peer_id": "a", "metrics": {"r":1}},
            {"peer_id": "a", "metrics": {"r":2}}
        ]);
        let req = Request::builder()
            .method("POST")
            .uri("/ingest")
            .header("content-type", "application/json")
            .header("x-auth-token", "token")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let _ = app.clone().oneshot(req).await.unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/peer/a")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let vals: Vec<(u64, serde_json::Value)> = serde_json::from_slice(&body).unwrap();
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].1["r"].as_f64().unwrap() as i64, 3);
    }

    #[tokio::test]
    async fn persists_and_prunes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("m.json");
        {
            let state = AppState::new("t".into(), &path, 1);
            let app = router(state.clone());
            let payload = serde_json::json!([{ "peer_id": "p", "metrics": {"v": 1}}]);
            let req = Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("content-type", "application/json")
                .header("x-auth-token", "t")
                .body(Body::from(payload.to_string()))
                .unwrap();
            let _ = app.oneshot(req).await.unwrap();
        }
        // Reload and ensure data persisted
        let state = AppState::new("t".into(), &path, 1);
        {
            let map = state.data.lock().unwrap();
            assert!(map.contains_key("p"));
        }
        // Insert artificially old data and prune
        {
            let mut map = state.data.lock().unwrap();
            if let Some(deque) = map.get_mut("p") {
                if let Some(entry) = deque.front_mut() {
                    entry.0 = 0; // timestamp far in past
                }
            }
        }
        state.prune();
        let map = state.data.lock().unwrap();
        assert!(map.get("p").map(|d| d.is_empty()).unwrap_or(true));
    }
}
