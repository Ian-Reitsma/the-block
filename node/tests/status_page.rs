#![cfg(feature = "integration-tests")]
#![cfg(feature = "gateway")]

use httpd::Method;
use runtime::net::TcpListener;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use the_block::http_client;
use the_block::web::status;
use the_block::Blockchain;

#[test]
fn status_returns_height() {
    runtime::block_on(async {
        let dir = sys::tempfile::tempdir().unwrap();
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
        let body = http_client::http_client()
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
