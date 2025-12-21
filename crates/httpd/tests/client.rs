use foundation_serialization::json::{self, Value};
use httpd::{BlockingClient, HttpClient, Method};
use std::io::Write;
use std::thread;

mod support;
use support::{LOCAL_BIND_ADDR, bind_std_listener};

#[test]
fn async_get_returns_text() {
    let listener = match bind_std_listener(LOCAL_BIND_ADDR) {
        Some(listener) => listener,
        None => return,
    };
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let body = "hello world";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let url = format!("http://{}", addr);
    let client = HttpClient::default();
    let response = runtime::block_on(async {
        client
            .request(Method::Get, &url)
            .expect("request")
            .send()
            .await
            .expect("response")
    });
    assert!(response.status().is_success());
    assert_eq!(response.text().unwrap(), "hello world");
}

#[test]
fn blocking_client_parses_json() {
    let listener = match bind_std_listener(LOCAL_BIND_ADDR) {
        Some(listener) => listener,
        None => return,
    };
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let body = r#"{"value": 7}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let url = format!("http://{}", addr);
    let client = BlockingClient::default();
    let response = client
        .request(Method::Get, &url)
        .expect("request")
        .send()
        .expect("response");
    let payload: Value = response.json().expect("json");
    match payload {
        Value::Object(map) => {
            let raw_value = map.get("value").cloned().expect("value field");
            let value: u64 = json::from_value(raw_value).expect("decode value");
            assert_eq!(value, 7);
        }
        other => panic!("unexpected payload: {:?}", other),
    }
}
