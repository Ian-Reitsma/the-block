#![allow(unsafe_code)]

use jni::objects::{GlobalRef, JObject, JValue};
use jni::JavaVM;
use ndk_context::android_context;
use std::pin::Pin;
use tracing::debug;

use super::{DeviceStatus, DeviceStatusProbe, ProbeError};

pub struct AndroidProbe {
    vm: JavaVM,
    context: GlobalRef,
}

impl AndroidProbe {
    pub fn new() -> Result<Self, ProbeError> {
        let ctx = android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
            .map_err(|err| ProbeError::backend(format!("vm attach: {err}")))?;
        let env = vm
            .attach_current_thread()
            .map_err(|err| ProbeError::backend(format!("attach thread: {err}")))?;
        let context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };
        let global = env
            .new_global_ref(context)
            .map_err(|err| ProbeError::backend(format!("global ref: {err}")))?;
        Ok(Self {
            vm,
            context: global,
        })
    }

    fn with_env<F, R>(&self, f: F) -> Result<R, ProbeError>
    where
        F: FnOnce(jni::JNIEnv<'_>, JObject<'_>) -> Result<R, ProbeError>,
    {
        let env = self
            .vm
            .attach_current_thread()
            .map_err(|err| ProbeError::backend(format!("attach thread: {err}")))?;
        let ctx = self.context.as_obj();
        f(env, ctx)
    }
}

impl DeviceStatusProbe for AndroidProbe {
    fn poll_status(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DeviceStatus, ProbeError>> + Send + '_>>
    {
        Box::pin(async move {
            self.with_env(|env, context| unsafe {
                let context_class = env
                    .find_class("android/content/Context")
                    .map_err(|err| ProbeError::backend(format!("context class: {err}")))?;

            let connectivity_service = env
                .get_static_field(
                    context_class,
                    "CONNECTIVITY_SERVICE",
                    "Ljava/lang/String;",
                )
                .map_err(|err| ProbeError::backend(format!("connectivity field: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("connectivity field obj: {err}")))?;

            let connectivity = env
                .call_method(
                    context,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[JValue::Object(connectivity_service)],
                )
                .map_err(|err| ProbeError::backend(format!("connectivity service: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("connectivity obj: {err}")))?;

            let network = env
                .call_method(
                    connectivity,
                    "getActiveNetwork",
                    "()Landroid/net/Network;",
                    &[],
                )
                .map_err(|err| ProbeError::backend(format!("active network: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("network obj: {err}")))?;

            let wifi = if network.is_null() {
                false
            } else {
                let capabilities = env
                    .call_method(
                        connectivity,
                        "getNetworkCapabilities",
                        "(Landroid/net/Network;)Landroid/net/NetworkCapabilities;",
                        &[JValue::Object(network)],
                    )
                    .map_err(|err| ProbeError::backend(format!("network capabilities: {err}")))?
                    .l()
                    .map_err(|err| ProbeError::backend(format!("capabilities obj: {err}")))?;
                if capabilities.is_null() {
                    false
                } else {
                    let wifi_id = env
                        .get_static_field(
                            "android/net/NetworkCapabilities",
                            "TRANSPORT_WIFI",
                            "I",
                        )
                        .map_err(|err| ProbeError::backend(format!("wifi transport id: {err}")))?
                        .i()
                        .map_err(|err| ProbeError::backend(format!("wifi transport value: {err}")))?;
                    env.call_method(
                        capabilities,
                        "hasTransport",
                        "(I)Z",
                        &[JValue::Int(wifi_id)],
                    )
                    .map_err(|err| ProbeError::backend(format!("hasTransport: {err}")))?
                    .z()
                    .map_err(|err| ProbeError::backend(format!("hasTransport bool: {err}")))?
                }
            };

            let intent_action = env
                .get_static_field(
                    "android/content/Intent",
                    "ACTION_BATTERY_CHANGED",
                    "Ljava/lang/String;",
                )
                .map_err(|err| ProbeError::backend(format!("battery action: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("battery action obj: {err}")))?;

            let filter = env
                .new_object(
                    "android/content/IntentFilter",
                    "(Ljava/lang/String;)V",
                    &[JValue::Object(intent_action)],
                )
                .map_err(|err| ProbeError::backend(format!("intent filter: {err}")))?;

            let intent = env
                .call_method(
                    context,
                    "registerReceiver",
                    "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;",
                    &[JValue::Object(JObject::null()), JValue::Object(filter)],
                )
                .map_err(|err| ProbeError::backend(format!("registerReceiver: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("intent obj: {err}")))?;

            let battery_class = env
                .find_class("android/os/BatteryManager")
                .map_err(|err| ProbeError::backend(format!("battery manager class: {err}")))?;

            let status_extra = env
                .get_static_field(
                    battery_class,
                    "EXTRA_STATUS",
                    "Ljava/lang/String;",
                )
                .map_err(|err| ProbeError::backend(format!("battery status extra: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("status extra obj: {err}")))?;

            let status = if intent.is_null() {
                -1
            } else {
                env.call_method(
                    intent,
                    "getIntExtra",
                    "(Ljava/lang/String;I)I",
                    &[JValue::Object(status_extra), JValue::Int(-1)],
                )
                .map_err(|err| ProbeError::backend(format!("intent status extra: {err}")))?
                .i()
                .map_err(|err| ProbeError::backend(format!("status value: {err}")))?
            };

            let charging = if status >= 0 {
                let charging_id = env
                    .get_static_field(battery_class, "BATTERY_STATUS_CHARGING", "I")
                    .map_err(|err| ProbeError::backend(format!("charging field: {err}")))?
                    .i()
                    .map_err(|err| ProbeError::backend(format!("charging value: {err}")))?;
                let full_id = env
                    .get_static_field(battery_class, "BATTERY_STATUS_FULL", "I")
                    .map_err(|err| ProbeError::backend(format!("full field: {err}")))?
                    .i()
                    .map_err(|err| ProbeError::backend(format!("full value: {err}")))?;
                status == charging_id || status == full_id
            } else {
                false
            };

            let property_capacity = env
                .get_static_field(
                    battery_class,
                    "BATTERY_PROPERTY_CAPACITY",
                    "I",
                )
                .map_err(|err| ProbeError::backend(format!("capacity property: {err}")))?
                .i()
                .map_err(|err| ProbeError::backend(format!("capacity value: {err}")))?;

            let battery_service = env
                .get_static_field(context_class, "BATTERY_SERVICE", "Ljava/lang/String;")
                .map_err(|err| ProbeError::backend(format!("battery service field: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("battery service obj: {err}")))?;

            let battery_manager = env
                .call_method(
                    context,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[JValue::Object(battery_service)],
                )
                .map_err(|err| ProbeError::backend(format!("battery manager service: {err}")))?
                .l()
                .map_err(|err| ProbeError::backend(format!("battery manager obj: {err}")))?;

            let capacity = match env.call_method(
                battery_manager,
                "getIntProperty",
                "(I)I",
                &[JValue::Int(property_capacity)],
            ) {
                Ok(value) => match value.i() {
                    Ok(v) => v,
                    Err(err) => {
                        debug!(
                            target: "light_client_device",
                            error = %err,
                            "capacity property conversion failed"
                        );
                        -1
                    }
                },
                Err(err) => {
                    debug!(
                        target: "light_client_device",
                        error = %err,
                        "capacity property read failed"
                    );
                    -1
                }
            };

            let level = if capacity >= 0 {
                capacity as f32 / 100.0
            } else if intent.is_null() {
                0.0
            } else {
                let level_extra = env
                    .get_static_field(
                        battery_class,
                        "EXTRA_LEVEL",
                        "Ljava/lang/String;",
                    )
                    .map_err(|err| ProbeError::backend(format!("level extra: {err}")))?
                    .l()
                    .map_err(|err| ProbeError::backend(format!("level extra obj: {err}")))?;
                let scale_extra = env
                    .get_static_field(
                        battery_class,
                        "EXTRA_SCALE",
                        "Ljava/lang/String;",
                    )
                    .map_err(|err| ProbeError::backend(format!("scale extra: {err}")))?
                    .l()
                    .map_err(|err| ProbeError::backend(format!("scale extra obj: {err}")))?;
                let level_value = env
                    .call_method(
                        intent,
                        "getIntExtra",
                        "(Ljava/lang/String;I)I",
                        &[JValue::Object(level_extra), JValue::Int(-1)],
                    )
                    .map_err(|err| ProbeError::backend(format!("level extra read: {err}")))?
                    .i()
                    .map_err(|err| ProbeError::backend(format!("level value: {err}")))?;
                let scale_value = env
                    .call_method(
                        intent,
                        "getIntExtra",
                        "(Ljava/lang/String;I)I",
                        &[JValue::Object(scale_extra), JValue::Int(100)],
                    )
                    .map_err(|err| ProbeError::backend(format!("scale extra read: {err}")))?
                    .i()
                    .map_err(|err| ProbeError::backend(format!("scale value: {err}")))?;
                if scale_value > 0 {
                    level_value as f32 / scale_value as f32
                } else {
                    0.0
                }
            };

            Ok(DeviceStatus {
                on_wifi: wifi,
                is_charging: charging,
                battery_level: level,
            })
        })
        })
    }
}
