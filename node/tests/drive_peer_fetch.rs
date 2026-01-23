use base64_fp::encode_standard;
use httpd::{Response, Router, ServerConfig, StatusCode};
use runtime;
use std::{env, fs, net::SocketAddr, sync::Arc};
use sys::tempfile::tempdir;
use the_block::{drive::DriveStore, net, rpc};

#[test]
fn drive_put_generates_share_url_and_stores_file() {
    let dir = tempdir().expect("create tempdir");
    let base_dir = dir.path().join("drive");
    env::set_var("TB_DRIVE_BASE_DIR", base_dir.to_string_lossy().to_string());
    env::set_var("TB_GATEWAY_URL", "https://gateway.example.block");
    let payload = b"drive-lite-sample";
    let encoded = encode_standard(payload);
    let resp = rpc::storage::drive_put(&encoded);
    let object_id = resp
        .get("object_id")
        .and_then(|value| value.as_str())
        .expect("object id present");
    assert_eq!(object_id.len(), 64);
    let share_url = resp
        .get("share_url")
        .and_then(|value| value.as_str())
        .expect("share url present");
    assert!(share_url.contains(object_id));
    assert!(share_url.starts_with("https://gateway.example.block/drive/"));
    let stored = fs::read(base_dir.join(object_id)).expect("file saved");
    assert_eq!(stored, payload.as_ref());
    for var in &["TB_DRIVE_BASE_DIR", "TB_GATEWAY_URL"] {
        env::remove_var(var);
    }
}

#[test]
fn drive_peer_fetch_falls_back_on_peer_failure() {
    runtime::block_on(async {
        let primary_dir = tempdir().expect("create tempdir");
        let primary_store = DriveStore::with_base(primary_dir.path().join("drive"));
        let payload = b"drive-peer-data".to_vec();
        let object_id = primary_store.store(&payload).expect("store primary object");

        let (server_handle, peer_addr) =
            spawn_drive_peer_server(Arc::new(payload.clone()), object_id.clone()).await;

        let secondary_dir = tempdir().expect("create secondary tempdir");
        env::set_var(
            "TB_DRIVE_BASE_DIR",
            secondary_dir
                .path()
                .join("drive")
                .to_string_lossy()
                .to_string(),
        );
        env::set_var(
            "TB_DRIVE_PEERS",
            format!("http://127.0.0.1:1,http://{}", peer_addr),
        );
        env::set_var("TB_DRIVE_ALLOW_PEER_FETCH", "1");
        env::set_var("TB_DRIVE_FETCH_TIMEOUT_MS", "500");

        let secondary_store = DriveStore::from_env();
        let fetched = secondary_store
            .fetch(&object_id)
            .expect("remote fetch succeeded");
        assert_eq!(fetched, payload);
        let cached = secondary_store
            .fetch(&object_id)
            .expect("local cache fetch succeeded");
        assert_eq!(cached, payload);

        for var in &[
            "TB_DRIVE_BASE_DIR",
            "TB_DRIVE_PEERS",
            "TB_DRIVE_ALLOW_PEER_FETCH",
            "TB_DRIVE_FETCH_TIMEOUT_MS",
        ] {
            env::remove_var(var);
        }

        server_handle.abort();
        let _ = server_handle.await;
    });
}

async fn spawn_drive_peer_server(
    data: Arc<Vec<u8>>,
    object_id: String,
) -> (runtime::JoinHandle<()>, SocketAddr) {
    let bind_addr: SocketAddr = "127.0.0.1:0".parse().expect("parse bind address");
    let listener =
        net::listener::bind_runtime("drive-peer", "drive_peer_listener_bind_failed", bind_addr)
            .await
            .expect("bind drive peer listener");
    let server_addr = listener.local_addr().expect("listener address");
    let router = Router::new(()).get("/drive/:object_id", {
        let data = Arc::clone(&data);
        let expected = object_id.clone();
        move |req| {
            let data = Arc::clone(&data);
            let expected = expected.clone();
            async move {
                if req.param("object_id") == Some(expected.as_str()) {
                    Ok(Response::new(StatusCode::OK).with_body(data.to_vec()))
                } else {
                    Ok(Response::new(StatusCode::NOT_FOUND))
                }
            }
        }
    });
    let config = ServerConfig::default();
    let handle = runtime::spawn(async move {
        let _ = httpd::serve(listener, router, config).await;
    });
    (handle, server_addr)
}
