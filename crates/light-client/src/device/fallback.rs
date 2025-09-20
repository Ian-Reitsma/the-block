use async_trait::async_trait;

use super::{DeviceFallback, DeviceStatus, DeviceStatusProbe, ProbeError};

#[derive(Default)]
pub struct DesktopProbe;

impl DesktopProbe {
    pub fn new() -> Result<Self, ProbeError> {
        let _ = DeviceFallback::default();
        Ok(Self)
    }
}

#[async_trait]
impl DeviceStatusProbe for DesktopProbe {
    async fn poll_status(&self) -> Result<DeviceStatus, ProbeError> {
        Err(ProbeError::not_available(
            "desktop builds rely on fallback device policy",
        ))
    }
}
