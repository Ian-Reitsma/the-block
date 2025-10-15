use diagnostics::tracing::debug;
use std::pin::Pin;

use super::{DeviceStatus, DeviceStatusProbe, ProbeError};

pub struct AndroidProbe;

impl AndroidProbe {
    pub fn new() -> Result<Self, ProbeError> {
        Ok(Self)
    }
}

impl DeviceStatusProbe for AndroidProbe {
    fn poll_status(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DeviceStatus, ProbeError>> + Send + '_>>
    {
        Box::pin(async move {
            let wifi = match sys::device::network::wifi_connected() {
                Ok(value) => value,
                Err(err) => {
                    debug!(target: "light_client_device", error = %err, "wifi detection failed");
                    false
                }
            };

            let is_charging = match sys::device::battery::is_charging() {
                Ok(value) => value,
                Err(err) => {
                    debug!(target: "light_client_device", error = %err, "charging status unavailable");
                    false
                }
            };

            let battery_level = match sys::device::battery::capacity_percent() {
                Ok(percent) => (percent as f32) / 100.0,
                Err(err) => {
                    debug!(target: "light_client_device", error = %err, "battery capacity unavailable");
                    0.0
                }
            };

            Ok(DeviceStatus {
                on_wifi: wifi,
                is_charging,
                battery_level,
            })
        })
    }
}
