use concurrency::Lazy;
use foundation_serialization::toml;
use foundation_serialization::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

const EMBEDDED_LOCALNET_DEVICES: &str = include_str!("../../../config/localnet_devices.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum DeviceClass {
    #[serde(alias = "phone")]
    Phone = 0,
    #[serde(alias = "laptop")]
    Laptop = 1,
    #[serde(alias = "router")]
    Router = 2,
}

#[derive(Debug, Deserialize)]
struct Threshold {
    rssi_min: i8,
    rtt_max_ms: u32,
}

#[derive(Debug)]
struct ProximityTable(HashMap<DeviceClass, Threshold>);

impl ProximityTable {
    fn load() -> Self {
        let path: PathBuf =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/localnet_devices.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| EMBEDDED_LOCALNET_DEVICES.to_string());
        let map: HashMap<DeviceClass, Threshold> = toml::from_str(&text).unwrap_or_default();
        Self(map)
    }

    fn validate(&self, class: DeviceClass, rssi: i8, rtt_ms: u32) -> bool {
        self.0
            .get(&class)
            .map(|t| rssi >= t.rssi_min && rtt_ms <= t.rtt_max_ms)
            .unwrap_or(false)
    }
}

static TABLE: Lazy<ProximityTable> = Lazy::new(ProximityTable::load);

pub fn validate_proximity(class: DeviceClass, rssi: i8, rtt_ms: u32) -> bool {
    TABLE.validate(class, rssi, rtt_ms)
}
