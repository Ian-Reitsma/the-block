use std::io;
use std::io::ErrorKind;

pub const LOCAL_BIND_ADDR: &str = "127.0.0.1:0";

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
