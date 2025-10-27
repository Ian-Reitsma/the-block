#![allow(dead_code)]

use diagnostics::tracing::warn;
use runtime::net::TcpListener as RuntimeTcpListener;
use std::io;
use std::net::{SocketAddr, TcpListener};

fn log_bind_failure(target: &'static str, event: &'static str, addr: SocketAddr, err: &io::Error) {
    warn!(target: target, error = %err, %addr, event = event, "{}", event);
}

pub fn bind_sync(
    target: &'static str,
    event: &'static str,
    addr: SocketAddr,
) -> io::Result<TcpListener> {
    match TcpListener::bind(addr) {
        Ok(listener) => Ok(listener),
        Err(err) => {
            log_bind_failure(target, event, addr, &err);
            Err(err)
        }
    }
}

pub async fn bind_runtime(
    target: &'static str,
    event: &'static str,
    addr: SocketAddr,
) -> io::Result<RuntimeTcpListener> {
    match RuntimeTcpListener::bind(addr).await {
        Ok(listener) => Ok(listener),
        Err(err) => {
            log_bind_failure(target, event, addr, &err);
            Err(err)
        }
    }
}
