use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use runtime::sync::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[cfg(feature = "telemetry")]
mod telemetry;
#[cfg(feature = "telemetry")]
pub use telemetry::{LIGHT_CLIENT_DEVICE_STATUS, REGISTRY as DEVICE_TELEMETRY_REGISTRY};

#[cfg(all(target_os = "android", feature = "android-probe"))]
mod android;
#[cfg(any(
    feature = "desktop-probe",
    not(any(feature = "android-probe", feature = "ios-probe"))
))]
mod fallback;
#[cfg(all(target_os = "ios", feature = "ios-probe"))]
mod ios;

#[cfg(all(target_os = "android", feature = "android-probe"))]
pub use android::AndroidProbe;
#[cfg(any(
    feature = "desktop-probe",
    not(any(feature = "android-probe", feature = "ios-probe"))
))]
pub use fallback::DesktopProbe;
#[cfg(all(target_os = "ios", feature = "ios-probe"))]
pub use ios::IosProbe;

pub type DynDeviceStatusProbe = Arc<dyn DeviceStatusProbe>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DeviceStatus {
    pub on_wifi: bool,
    pub is_charging: bool,
    pub battery_level: f32,
}

impl DeviceStatus {
    pub fn clamp(mut self) -> Self {
        if !self.battery_level.is_finite() {
            self.battery_level = 0.0;
        }
        self.battery_level = self.battery_level.clamp(0.0, 1.0);
        self
    }
}

impl From<DeviceFallback> for DeviceStatus {
    fn from(value: DeviceFallback) -> Self {
        Self {
            on_wifi: value.on_wifi,
            is_charging: value.is_charging,
            battery_level: value.battery_level,
        }
        .clamp()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DeviceFallback {
    pub on_wifi: bool,
    pub is_charging: bool,
    pub battery_level: f32,
}

impl Default for DeviceFallback {
    fn default() -> Self {
        Self {
            on_wifi: true,
            is_charging: true,
            battery_level: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceStatusFreshness {
    Fresh,
    Cached,
    Fallback,
}

impl DeviceStatusFreshness {
    pub fn as_label(self) -> &'static str {
        match self {
            DeviceStatusFreshness::Fresh => "fresh",
            DeviceStatusFreshness::Cached => "cached",
            DeviceStatusFreshness::Fallback => "fallback",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceStatusSnapshot {
    pub status: DeviceStatus,
    pub observed_at: SystemTime,
    pub freshness: DeviceStatusFreshness,
    pub stale_for: Duration,
}

impl DeviceStatusSnapshot {
    fn fresh(status: DeviceStatus) -> Self {
        Self {
            status,
            observed_at: SystemTime::now(),
            freshness: DeviceStatusFreshness::Fresh,
            stale_for: Duration::from_secs(0),
        }
    }

    fn cached(status: DeviceStatus, observed_at: SystemTime, stale_for: Duration) -> Self {
        Self {
            status,
            observed_at,
            freshness: DeviceStatusFreshness::Cached,
            stale_for,
        }
    }

    fn fallback(status: DeviceStatus) -> Self {
        Self {
            status,
            observed_at: SystemTime::now(),
            freshness: DeviceStatusFreshness::Fallback,
            stale_for: Duration::from_secs(0),
        }
    }
}

#[derive(Debug)]
pub struct ProbeError {
    message: String,
}

impl ProbeError {
    pub fn not_available(reason: &str) -> Self {
        Self {
            message: format!("probe unavailable: {reason}"),
        }
    }

    pub fn backend(err: impl fmt::Display) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProbeError {}

#[async_trait]
pub trait DeviceStatusProbe: Send + Sync {
    async fn poll_status(&self) -> Result<DeviceStatus, ProbeError>;
}

pub trait IntoDynProbe {
    fn into_dyn(self) -> DynDeviceStatusProbe
    where
        Self: Sized + DeviceStatusProbe + 'static,
    {
        Arc::new(self)
    }
}

impl<T> IntoDynProbe for T where T: DeviceStatusProbe + 'static {}

struct CachedStatus {
    status: DeviceStatus,
    observed_at: SystemTime,
    monotonic: Instant,
}

pub struct DeviceStatusWatcher {
    probe: DynDeviceStatusProbe,
    fallback: DeviceFallback,
    stale_after: Duration,
    last: Mutex<Option<CachedStatus>>,
}

impl DeviceStatusWatcher {
    pub fn new(
        probe: DynDeviceStatusProbe,
        fallback: DeviceFallback,
        stale_after: Duration,
    ) -> Self {
        Self {
            probe,
            fallback,
            stale_after,
            last: Mutex::new(None),
        }
    }

    pub async fn poll(&self) -> DeviceStatusSnapshot {
        match self.probe.poll_status().await.map(DeviceStatus::clamp) {
            Ok(status) => {
                let snapshot = DeviceStatusSnapshot::fresh(status);
                let mut guard = self.last.lock().await;
                *guard = Some(CachedStatus {
                    status,
                    observed_at: snapshot.observed_at,
                    monotonic: Instant::now(),
                });
                drop(guard);
                #[cfg(feature = "telemetry")]
                telemetry::record(&snapshot);
                snapshot
            }
            Err(err) => {
                warn!(target: "light_client_device", error = %err, "device probe failed");
                let mut guard = self.last.lock().await;
                if let Some(cache) = guard.as_ref() {
                    let age = cache.monotonic.elapsed();
                    if age <= self.stale_after {
                        let snapshot =
                            DeviceStatusSnapshot::cached(cache.status, cache.observed_at, age);
                        #[cfg(feature = "telemetry")]
                        telemetry::record(&snapshot);
                        debug!(
                            target: "light_client_device",
                            age_secs = age.as_secs_f32(),
                            "using cached device status"
                        );
                        return snapshot;
                    }
                }
                let fallback_status: DeviceStatus = self.fallback.into();
                let snapshot = DeviceStatusSnapshot::fallback(fallback_status);
                *guard = Some(CachedStatus {
                    status: fallback_status,
                    observed_at: snapshot.observed_at,
                    monotonic: Instant::now(),
                });
                drop(guard);
                #[cfg(feature = "telemetry")]
                telemetry::record(&snapshot);
                snapshot
            }
        }
    }
}

#[cfg(all(target_os = "android", feature = "android-probe"))]
fn make_default_probe() -> Result<DynDeviceStatusProbe, ProbeError> {
    Ok(AndroidProbe::new()?.into_dyn())
}

#[cfg(all(target_os = "ios", feature = "ios-probe"))]
fn make_default_probe() -> Result<DynDeviceStatusProbe, ProbeError> {
    Ok(IosProbe::new()?.into_dyn())
}

#[cfg(any(
    feature = "desktop-probe",
    not(any(feature = "android-probe", feature = "ios-probe"))
))]
fn make_default_probe() -> Result<DynDeviceStatusProbe, ProbeError> {
    Ok(DesktopProbe::new()?.into_dyn())
}

#[cfg(not(any(
    all(target_os = "android", feature = "android-probe"),
    all(target_os = "ios", feature = "ios-probe"),
    feature = "desktop-probe",
    not(any(feature = "android-probe", feature = "ios-probe"))
)))]
fn make_default_probe() -> Result<DynDeviceStatusProbe, ProbeError> {
    Err(ProbeError::not_available(
        "no probe compiled for this target",
    ))
}

pub fn default_probe() -> Result<DynDeviceStatusProbe, ProbeError> {
    make_default_probe()
}
