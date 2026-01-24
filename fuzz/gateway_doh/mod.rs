#![forbid(unsafe_code)]

use the_block::web::gateway::parse_dns_packet;

pub fn run(data: &[u8]) {
    if let Some(question) = parse_dns_packet(data) {
        let _ = question.name.len();
        #[allow(unused_must_use)]
        let _ = question.record_type.as_u16();
    }
}
