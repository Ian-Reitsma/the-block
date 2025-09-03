#[cfg(feature="gateway")]
use the_block::web::gateway::ip_key;

#[cfg(feature="gateway")]
#[test]
fn ip_key_stable() {
    use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    let ip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1,2,3,4)), 80);
    assert_eq!(ip_key(&ip), 0x04030201);
}
