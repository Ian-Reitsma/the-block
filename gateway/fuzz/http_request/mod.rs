#![forbid(unsafe_code)]

use the_block::gateway::http::parse_request;

pub fn run(data: &[u8]) {
    parse_request(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minimal_request() {
        let request = b"GET / HTTP/1.1\r\n\r\n";
        run(request);
    }

    #[test]
    fn tolerates_empty_payload() {
        run(&[]);
    }
}
