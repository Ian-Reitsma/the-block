use std::future::Future;
use tokio::time::{timeout, Duration};

#[allow(dead_code)]
pub async fn expect_timeout<F, T>(fut: F) -> T
where
    F: Future<Output = T>,
{
    timeout(Duration::from_secs(10), fut)
        .await
        .expect("operation timed out")
}
