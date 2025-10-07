use crate::Blockchain;
use httpd::{serve, HttpError, Method, Request, Response, Router, ServerConfig, StatusCode};
use runtime::net::TcpListener;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub async fn run(addr: SocketAddr, bc: Arc<Mutex<Blockchain>>) -> diagnostics::anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let state = StatusState { bc };
    let router = Router::new(state).route(Method::Get, "/", status_handler);
    serve(listener, router, ServerConfig::default()).await?;
    Ok(())
}

struct StatusState {
    bc: Arc<Mutex<Blockchain>>,
}

async fn status_handler(req: Request<StatusState>) -> Result<Response, HttpError> {
    let height = req.state().bc.lock().unwrap().block_height;
    let body = format!("height: {height}\n").into_bytes();
    Ok(Response::new(StatusCode::OK).with_body(body))
}
