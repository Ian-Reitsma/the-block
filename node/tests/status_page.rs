#![cfg(feature = "integration-tests")]
#![cfg(feature = "gateway")]

use std::sync::{Arc, Mutex};
use tempfile;
use the_block::web::status;
use the_block::Blockchain;

#[tokio::test]
async fn status_returns_height() {
    let dir = tempfile::tempdir().unwrap();
    let mut bc = Blockchain::new(dir.path().to_str().unwrap());
    bc.block_height = 42;
    let bc = Arc::new(Mutex::new(bc));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        status::run(addr, bc).await.unwrap();
    });
    let url = format!("http://{}", addr);
    let body = reqwest::get(url).await.unwrap().text().await.unwrap();
    assert!(body.contains("42"));
}
