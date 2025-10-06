use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use light_client::{
    sync_background_with_probe, DeviceFallback, DeviceStatus, DeviceStatusProbe,
    DeviceStatusWatcher, GatingReason, Header, LightClient, LightClientConfig, ProbeError,
    SyncOptions,
};

#[cfg(feature = "telemetry")]
use light_client::LIGHT_CLIENT_DEVICE_STATUS;

fn make_header(prev: &Header, height: u64) -> Header {
    let mut h = Header {
        height,
        prev_hash: prev.hash(),
        merkle_root: [0u8; 32],
        checkpoint_hash: [0u8; 32],
        validator_key: None,
        checkpoint_sig: None,
        nonce: 0,
        difficulty: 1,
        timestamp_millis: 0,
        l2_roots: vec![],
        l2_sizes: vec![],
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: vec![],
    };
    loop {
        let hash = h.hash();
        let v = u64::from_le_bytes(hash[..8].try_into().unwrap());
        if v <= u64::MAX / h.difficulty {
            break;
        }
        h.nonce = h.nonce.wrapping_add(1);
    }
    h
}

struct MockProbe {
    responses: Mutex<VecDeque<Result<DeviceStatus, ProbeError>>>,
}

impl MockProbe {
    fn new(responses: Vec<Result<DeviceStatus, ProbeError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

#[async_trait]
impl DeviceStatusProbe for MockProbe {
    async fn poll_status(&self) -> Result<DeviceStatus, ProbeError> {
        let mut guard = self.responses.lock().unwrap();
        guard.pop_front().unwrap_or_else(|| {
            Ok(DeviceStatus {
                on_wifi: true,
                is_charging: true,
                battery_level: 1.0,
            })
        })
    }
}

#[test]
fn gating_on_wifi_requirement() {
    runtime::block_on(async {
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: vec![],
            l2_sizes: vec![],
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: vec![],
        };
        let probe = MockProbe::new(vec![Ok(DeviceStatus {
            on_wifi: false,
            is_charging: true,
            battery_level: 1.0,
        })]);
        let watcher = DeviceStatusWatcher::new(
            Arc::new(probe),
            DeviceFallback::default(),
            Duration::from_secs(30),
        );
        let mut client = LightClient::new(genesis.clone());
        let header = make_header(&genesis, 1);
        let batches = Arc::new(Mutex::new(VecDeque::from([
            vec![header.clone()],
            Vec::new(),
        ])));
        let fetch_batches = batches.clone();
        let fetch = move |_start: u64, _batch: usize| {
            let fetch_batches = fetch_batches.clone();
            async move {
                let mut guard = fetch_batches.lock().unwrap();
                guard.pop_front().unwrap_or_default()
            }
        };
        let outcome = sync_background_with_probe(
            &mut client,
            SyncOptions {
                wifi_only: true,
                require_charging: true,
                min_battery: 0.0,
                ..SyncOptions::default()
            },
            &watcher,
            fetch,
        )
        .await;
        assert_eq!(outcome.appended, 0);
        assert_eq!(outcome.gating, Some(GatingReason::WifiUnavailable));
    });
}

#[test]
fn honors_charging_override_from_config() {
    runtime::block_on(async {
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: vec![],
            l2_sizes: vec![],
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: vec![],
        };
        let probe = MockProbe::new(vec![Ok(DeviceStatus {
            on_wifi: true,
            is_charging: false,
            battery_level: 1.0,
        })]);
        let watcher = DeviceStatusWatcher::new(
            Arc::new(probe),
            DeviceFallback::default(),
            Duration::from_secs(30),
        );
        let mut client = LightClient::new(genesis.clone());
        let header = make_header(&genesis, 1);
        let batches = Arc::new(Mutex::new(VecDeque::from([
            vec![header.clone()],
            Vec::new(),
        ])));
        let fetch_batches = batches.clone();
        let fetch = move |_start: u64, _batch: usize| {
            let fetch_batches = fetch_batches.clone();
            async move {
                let mut guard = fetch_batches.lock().unwrap();
                guard.pop_front().unwrap_or_default()
            }
        };
        let cfg = LightClientConfig {
            ignore_charging_requirement: true,
            ..LightClientConfig::default()
        };
        let outcome = sync_background_with_probe(
            &mut client,
            SyncOptions {
                wifi_only: true,
                require_charging: true,
                min_battery: 0.0,
                ..SyncOptions::default()
            }
            .apply_config(&cfg),
            &watcher,
            fetch,
        )
        .await;
        assert_eq!(outcome.gating, None);
        assert_eq!(client.tip_height(), 1);
    });
}

#[cfg(feature = "telemetry")]
#[test]
fn telemetry_records_device_status() {
    runtime::block_on(async {
        let genesis = Header {
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: [0u8; 32],
            checkpoint_hash: [0u8; 32],
            validator_key: None,
            checkpoint_sig: None,
            nonce: 0,
            difficulty: 1,
            timestamp_millis: 0,
            l2_roots: vec![],
            l2_sizes: vec![],
            vdf_commit: [0u8; 32],
            vdf_output: [0u8; 32],
            vdf_proof: vec![],
        };
        #[cfg(feature = "telemetry")]
        LIGHT_CLIENT_DEVICE_STATUS.reset();
        let probe = MockProbe::new(vec![
            Ok(DeviceStatus {
                on_wifi: true,
                is_charging: true,
                battery_level: 0.5,
            }),
            Ok(DeviceStatus {
                on_wifi: true,
                is_charging: true,
                battery_level: 0.5,
            }),
        ]);
        let watcher = DeviceStatusWatcher::new(
            Arc::new(probe),
            DeviceFallback::default(),
            Duration::from_secs(30),
        );
        let mut client = LightClient::new(genesis.clone());
        let header = make_header(&genesis, 1);
        let batches = Arc::new(Mutex::new(VecDeque::from([
            vec![header.clone()],
            Vec::new(),
        ])));
        let fetch_batches = batches.clone();
        let fetch = move |_start: u64, _batch: usize| {
            let fetch_batches = fetch_batches.clone();
            async move {
                let mut guard = fetch_batches.lock().unwrap();
                guard.pop_front().unwrap_or_default()
            }
        };
        let outcome = sync_background_with_probe(
            &mut client,
            SyncOptions {
                wifi_only: true,
                require_charging: true,
                min_battery: 0.0,
                ..SyncOptions::default()
            },
            &watcher,
            fetch,
        )
        .await;
        assert_eq!(outcome.appended, 1);
        assert_eq!(client.tip_height(), 1);
        #[cfg(feature = "telemetry")]
        {
            let metrics = LIGHT_CLIENT_DEVICE_STATUS.collect();
            assert_eq!(metrics.len(), 1);
            let sample = &metrics[0];
            assert_eq!(sample.get_label("freshness"), Some("fresh"));
        }
    });
}
