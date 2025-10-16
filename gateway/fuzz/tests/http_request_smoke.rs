#[path = "../http_request/mod.rs"]
mod http_request;

#[test]
fn parses_valid_request() {
    let request = b"POST /submit HTTP/1.1\r\ncontent-length: 3\r\n\r\nabc";
    http_request::run(request);
}

#[test]
fn handles_fragmented_payload() {
    http_request::run(b"INVALID");
}
