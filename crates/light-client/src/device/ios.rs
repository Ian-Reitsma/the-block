#![allow(unsafe_code)]

use diagnostics::tracing::debug;
use std::ffi::{c_void, CString};
use std::pin::Pin;

use super::{DeviceStatus, DeviceStatusProbe, ProbeError};

#[cfg(target_os = "ios")]
mod platform {
    use super::{debug, CString, DeviceStatus, ProbeError};
    use std::ffi::{c_char, c_void};

    type Id = *mut c_void;
    type Sel = *const c_void;
    type Bool = i8;
    type CFIndex = isize;
    type CFTypeRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;

    const YES: Bool = 1;

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> *const c_void;
        fn CFDictionaryGetCount(dict: CFDictionaryRef) -> CFIndex;
        fn CFRelease(value: CFTypeRef);
    }

    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn CNCopySupportedInterfaces() -> CFArrayRef;
        fn CNCopyCurrentNetworkInfo(interface_name: CFStringRef) -> CFDictionaryRef;
    }

    unsafe fn msg_send_id(receiver: Id, selector: Sel) -> Id {
        let func: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as *const ());
        func(receiver, selector)
    }

    unsafe fn msg_send_void_bool(receiver: Id, selector: Sel, arg: Bool) {
        let func: extern "C" fn(Id, Sel, Bool) = std::mem::transmute(objc_msgSend as *const ());
        func(receiver, selector, arg);
    }

    unsafe fn msg_send_i32(receiver: Id, selector: Sel) -> i32 {
        let func: extern "C" fn(Id, Sel) -> i32 = std::mem::transmute(objc_msgSend as *const ());
        func(receiver, selector)
    }

    unsafe fn msg_send_f32(receiver: Id, selector: Sel) -> f32 {
        let func: extern "C" fn(Id, Sel) -> f32 = std::mem::transmute(objc_msgSend as *const ());
        func(receiver, selector)
    }

    fn selector(name: &str) -> Sel {
        let c_name = CString::new(name).expect("selector name without interior null");
        unsafe { sel_registerName(c_name.as_ptr()) }
    }

    fn class(name: &str) -> Result<Id, ProbeError> {
        let c_name = CString::new(name).map_err(|_| ProbeError::backend("invalid class name"))?;
        let value = unsafe { objc_getClass(c_name.as_ptr()) };
        if value.is_null() {
            Err(ProbeError::backend(format!("class `{name}` not found")))
        } else {
            Ok(value)
        }
    }

    struct CfOwned<T> {
        ptr: T,
    }

    impl CfOwned<CFArrayRef> {
        fn from_array(ptr: CFArrayRef) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self { ptr })
            }
        }

        fn as_raw(&self) -> CFArrayRef {
            self.ptr
        }
    }

    impl CfOwned<CFDictionaryRef> {
        fn from_dict(ptr: CFDictionaryRef) -> Option<Self> {
            if ptr.is_null() {
                None
            } else {
                Some(Self { ptr })
            }
        }

        fn as_raw(&self) -> CFDictionaryRef {
            self.ptr
        }
    }

    impl Drop for CfOwned<CFArrayRef> {
        fn drop(&mut self) {
            unsafe { CFRelease(self.ptr as CFTypeRef) };
        }
    }

    impl Drop for CfOwned<CFDictionaryRef> {
        fn drop(&mut self) {
            unsafe { CFRelease(self.ptr as CFTypeRef) };
        }
    }

    fn wifi_connected() -> bool {
        unsafe {
            let interfaces = match CfOwned::from_array(CNCopySupportedInterfaces()) {
                Some(array) => array,
                None => return false,
            };
            let count = CFArrayGetCount(interfaces.as_raw());
            for index in 0..count {
                let interface = CFArrayGetValueAtIndex(interfaces.as_raw(), index);
                if interface.is_null() {
                    continue;
                }
                let info_ptr =
                    CfOwned::from_dict(CNCopyCurrentNetworkInfo(interface as CFStringRef));
                if let Some(info) = info_ptr {
                    let entries = CFDictionaryGetCount(info.as_raw());
                    if entries > 0 {
                        return true;
                    }
                }
            }
            false
        }
    }

    fn current_device() -> Result<Id, ProbeError> {
        let device_class = class("UIDevice")?;
        unsafe {
            let current_selector = selector("currentDevice");
            let device = msg_send_id(device_class, current_selector);
            if device.is_null() {
                return Err(ProbeError::backend("UIDevice.currentDevice returned null"));
            }
            let enable_selector = selector("setBatteryMonitoringEnabled:");
            msg_send_void_bool(device, enable_selector, YES);
            Ok(device)
        }
    }

    pub fn initialise() -> Result<(), ProbeError> {
        let _ = current_device()?;
        Ok(())
    }

    pub fn poll_status() -> Result<DeviceStatus, ProbeError> {
        unsafe {
            let device = current_device()?;
            let state_selector = selector("batteryState");
            let state = msg_send_i32(device, state_selector);
            let level_selector = selector("batteryLevel");
            let raw_level = msg_send_f32(device, level_selector);
            if raw_level < 0.0 {
                debug!(target: "light_client_device", "battery level unavailable");
            }
            let wifi = wifi_connected();
            Ok(DeviceStatus {
                on_wifi: wifi,
                is_charging: state == 2 || state == 3,
                battery_level: if raw_level.is_sign_negative() {
                    0.0
                } else {
                    raw_level
                },
            })
        }
    }
}

#[cfg(not(target_os = "ios"))]
mod platform {
    use super::{DeviceStatus, ProbeError};

    pub fn initialise() -> Result<(), ProbeError> {
        Err(ProbeError::not_available(
            "ios probe compiled on non-ios target",
        ))
    }

    pub fn poll_status() -> Result<DeviceStatus, ProbeError> {
        Err(ProbeError::not_available(
            "ios probe compiled on non-ios target",
        ))
    }
}

pub struct IosProbe;

impl IosProbe {
    pub fn new() -> Result<Self, ProbeError> {
        platform::initialise().map(|_| Self)
    }
}

impl DeviceStatusProbe for IosProbe {
    fn poll_status(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DeviceStatus, ProbeError>> + Send + '_>>
    {
        Box::pin(async move { platform::poll_status() })
    }
}
