use std::sync::{Arc, Mutex};

pub trait KycProvider: Send + Sync {
    fn verify(&self, user_id: &str) -> Result<bool, String>;
}

struct NoopKyc;

impl KycProvider for NoopKyc {
    fn verify(&self, _user_id: &str) -> Result<bool, String> {
        Ok(true)
    }
}

static PROVIDER: Mutex<Option<Arc<dyn KycProvider>>> = Mutex::new(None);

/// Install a custom KYC provider.
pub fn set_provider(p: Arc<dyn KycProvider>) {
    *PROVIDER.lock().unwrap() = Some(p);
}

fn provider() -> Arc<dyn KycProvider> {
    PROVIDER
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| Arc::new(NoopKyc))
}

/// Verify `user_id` against the configured provider.
pub fn verify(user_id: &str) -> Result<bool, String> {
    provider().verify(user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fail;
    impl KycProvider for Fail {
        fn verify(&self, _user_id: &str) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[test]
    fn default_allows() {
        PROVIDER.lock().unwrap().take();
        assert!(verify("alice").unwrap());
    }

    #[test]
    fn custom_provider_overrides() {
        PROVIDER.lock().unwrap().take();
        set_provider(Arc::new(Fail));
        assert!(!verify("alice").unwrap());
    }
}
