#![cfg(feature = "integration-tests")]
#![cfg(feature = "gateway")]

use httpd::{HttpClient, Method};
use runtime::net::TcpListener;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tempfile;
use the_block::web::status;
use the_block::Blockchain;

#[test]
fn status_returns_height() {
    runtime::block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let mut bc = Blockchain::new(dir.path().to_str().unwrap());
        bc.block_height = 42;
        let bc = Arc::new(Mutex::new(bc));
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(bind_addr).await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        the_block::spawn(async move {
            status::run(addr, bc).await.unwrap();
        });
        let url = format!("http://{}", addr);
        let body = HttpClient::default()
            .request(Method::Get, &url)
            .unwrap()
            .send()
            .await
            .unwrap()
            .text()
            .unwrap();
        assert!(body.contains("42"));
    });
}
