use foundation_lazy::sync::Lazy;
use runtime::telemetry::{GaugeVec, Opts, Registry};

use super::DeviceStatusSnapshot;

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

pub static LIGHT_CLIENT_DEVICE_STATUS: Lazy<GaugeVec> = Lazy::new(|| {
    let gauge = GaugeVec::new(
        Opts::new(
            "light_client_device_status",
            "Light client device probe readings",
        )
        .namespace("the_block"),
        &["field", "freshness"],
    );
    REGISTRY
        .register(Box::new(gauge.clone()))
        .expect("register device status gauge");
    gauge
});

pub fn record(snapshot: &DeviceStatusSnapshot) {
    let freshness = snapshot.freshness.as_label();
    LIGHT_CLIENT_DEVICE_STATUS
        .ensure_handle_for_label_values(&["wifi", freshness])
        .expect(runtime::telemetry::LABEL_REGISTRATION_ERR)
        .set(if snapshot.status.on_wifi { 1.0 } else { 0.0 });
    LIGHT_CLIENT_DEVICE_STATUS
        .ensure_handle_for_label_values(&["charging", freshness])
        .expect(runtime::telemetry::LABEL_REGISTRATION_ERR)
        .set(if snapshot.status.is_charging {
            1.0
        } else {
            0.0
        });
    LIGHT_CLIENT_DEVICE_STATUS
        .ensure_handle_for_label_values(&["battery", freshness])
        .expect(runtime::telemetry::LABEL_REGISTRATION_ERR)
        .set(snapshot.status.battery_level as f64);
}
