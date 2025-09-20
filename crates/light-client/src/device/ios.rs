#![allow(unsafe_code)]

use async_trait::async_trait;
use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use objc::runtime::{Object, YES};
use objc::{class, msg_send, sel, sel_impl};
use tracing::debug;

use super::{DeviceStatus, DeviceStatusProbe, ProbeError};

#[link(name = "SystemConfiguration", kind = "framework")]
extern "C" {
    fn CNCopySupportedInterfaces() -> core_foundation::array::CFArrayRef;
    fn CNCopyCurrentNetworkInfo(
        interface_name: core_foundation::string::CFStringRef,
    ) -> core_foundation::dictionary::CFDictionaryRef;
}

pub struct IosProbe;

impl IosProbe {
    pub fn new() -> Result<Self, ProbeError> {
        unsafe {
            let device: *mut Object = msg_send![class!(UIDevice), currentDevice];
            let _: () = msg_send![device, setBatteryMonitoringEnabled: YES];
        }
        Ok(Self)
    }
}

fn wifi_connected() -> bool {
    unsafe {
        let interfaces = CNCopySupportedInterfaces();
        if interfaces.is_null() {
            return false;
        }
        let array: CFArray<CFString> = CFArray::wrap_under_create_rule(interfaces);
        for idx in 0..array.len() {
            if let Some(interface) = array.get(idx) {
                let info = CNCopyCurrentNetworkInfo(interface.as_concrete_TypeRef());
                if !info.is_null() {
                    let dict: CFDictionary = CFDictionary::wrap_under_create_rule(info);
                    if dict.len() > 0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[async_trait]
impl DeviceStatusProbe for IosProbe {
    async fn poll_status(&self) -> Result<DeviceStatus, ProbeError> {
        unsafe {
            let device: *mut Object = msg_send![class!(UIDevice), currentDevice];
            let _: () = msg_send![device, setBatteryMonitoringEnabled: YES];
            let state: i32 = msg_send![device, batteryState];
            let level: f32 = msg_send![device, batteryLevel];
            if level < 0.0 {
                debug!(target: "light_client_device", "battery level unavailable");
            }
            let wifi = wifi_connected();
            Ok(DeviceStatus {
                on_wifi: wifi,
                is_charging: state == 2 || state == 3,
                battery_level: if level < 0.0 { 0.0 } else { level },
            })
        }
    }
}
