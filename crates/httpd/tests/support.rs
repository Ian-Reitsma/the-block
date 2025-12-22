use runtime::net::TcpListener;
use std::io;
use std::io::ErrorKind;
use std::net::SocketAddr;

pub const LOCAL_BIND_ADDR: &str = "127.0.0.1:0";

#[allow(dead_code)]
pub async fn bind_runtime_listener(addr: &str) -> Option<TcpListener> {
    let socket = addr.parse::<SocketAddr>().expect("valid socket address");
    match TcpListener::bind(socket).await {
        Ok(listener) => Some(listener),
        Err(err) => handle_bind_error(addr, err),
    }
}

#[allow(dead_code)]
pub fn bind_std_listener(addr: &str) -> Option<std::net::TcpListener> {
    match std::net::TcpListener::bind(addr) {
        Ok(listener) => Some(listener),
        Err(err) => handle_bind_error(addr, err),
    }
}

fn handle_bind_error<T>(addr: &str, err: io::Error) -> Option<T> {
    if err.kind() == ErrorKind::PermissionDenied {
        eprintln!(
            "Skipping HTTP test because binding {} is not permitted in this sandbox: {err}",
            addr
        );
        None
    } else {
        panic!("bind listener {}: {err}", addr);
    }
}
