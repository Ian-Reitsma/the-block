mod util;

use credits::CreditError;
use the_block::storage::fs::credit_err_to_io;

#[cfg(unix)]
#[test]
fn insufficient_maps_to_enospc() {
    util::rpc::randomize_client_timeout();
    let err = credit_err_to_io(CreditError::Insufficient);
    assert_eq!(err.raw_os_error(), Some(28));
}

#[cfg(windows)]
#[test]
fn insufficient_maps_to_disk_full() {
    util::rpc::randomize_client_timeout();
    let err = credit_err_to_io(CreditError::Insufficient);
    assert_eq!(err.raw_os_error(), Some(112));
}
