use std::pin::Pin;

use super::{DeviceFallback, DeviceStatus, DeviceStatusProbe, ProbeError};

#[derive(Default)]
pub struct DesktopProbe;

impl DesktopProbe {
    pub fn new() -> Result<Self, ProbeError> {
        let _ = DeviceFallback::default();
        Ok(Self)
    }
}

impl DeviceStatusProbe for DesktopProbe {
    fn poll_status(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DeviceStatus, ProbeError>> + Send + '_>>
    {
        Box::pin(async {
            Err(ProbeError::not_available(
                "desktop builds rely on fallback device policy",
            ))
        })
    }
}
