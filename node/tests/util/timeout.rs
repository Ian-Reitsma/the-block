#![cfg(feature = "integration-tests")]
use std::future::Future;
use std::time::Duration;

#[allow(dead_code)]
pub async fn expect_timeout<F, T>(fut: F) -> T
where
    F: Future<Output = T>,
{
    the_block::timeout(Duration::from_secs(60), fut)
        .await
        .expect("operation timed out")
}
