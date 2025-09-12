use crate::Blockchain;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub async fn run(addr: SocketAddr, bc: Arc<Mutex<Blockchain>>) -> anyhow::Result<()> {
    let make = make_service_fn(move |_conn| {
        let bc = Arc::clone(&bc);
        async move {
            Ok::<_, hyper::Error>(service_fn(move |_req: Request<Body>| {
                let height = bc.lock().unwrap().block_height;
                let body = format!("height: {height}\n");
                async move { Ok::<_, hyper::Error>(Response::new(Body::from(body))) }
            }))
        }
    });
    Server::bind(&addr).serve(make).await?;
    Ok(())
}
