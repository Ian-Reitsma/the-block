use std::path::PathBuf;

use concurrency::{mutex, MutexExt, MutexT};
use storage_market::{DiscoveryRequest, ProviderProfile, StorageMarket};
use sys::tempfile::tempdir;
use the_block::net::load_net_key;
use the_block::storage::provider_directory::{self, ProviderAdvertisement};

type MarketHandle = std::sync::Arc<MutexT<StorageMarket>>;

fn market_handle(path: PathBuf) -> MarketHandle {
    let market = StorageMarket::open(path).expect("open market");
    std::sync::Arc::new(mutex(market))
}

#[test]
fn discovers_remote_provider_from_advertisement() {
    let dir = tempdir().expect("tempdir");
    let market_path = dir.path().join("market");
    let key_path = dir.path().join("net_key");
    std::env::set_var("TB_NET_KEY_PATH", key_path.to_string_lossy().to_string());
    let handle = market_handle(market_path);
    provider_directory::install_directory(handle.clone());

    let mut remote = ProviderProfile::new("remote-1".into(), 8 * 1024, 5, 100);
    remote.region = Some("us-east".into());
    remote.version = 7;
    let key = load_net_key();
    let advert = ProviderAdvertisement::sign(remote.clone(), 60, &key);
    provider_directory::handle_advertisement(advert);

    let request = DiscoveryRequest {
        object_size: 512,
        shares: 2,
        region: Some("us-east".into()),
        max_price_per_block: Some(10),
        min_success_rate_ppm: None,
        limit: 5,
    };

    let providers = handle
        .guard()
        .discover_providers(request)
        .expect("discover providers");
    assert!(
        providers
            .iter()
            .any(|p| p.provider_id == remote.provider_id),
        "remote provider not discovered"
    );
    let remote_entry = providers
        .iter()
        .find(|p| p.provider_id == remote.provider_id)
        .expect("remote present");
    assert_eq!(remote_entry.version, remote.version);
}

#[test]
fn hydrates_directory_from_lookup_response() {
    let dir = tempdir().expect("tempdir");
    let market_path = dir.path().join("market");
    let key_path = dir.path().join("net_key_lookup");
    std::env::set_var("TB_NET_KEY_PATH", key_path.to_string_lossy().to_string());
    let handle = market_handle(market_path);
    provider_directory::install_directory(handle.clone());

    let mut remote = ProviderProfile::new("remote-lookup".into(), 16 * 1024, 7, 50);
    remote.version = 3;
    let key = load_net_key();
    let path = vec![key.verifying_key().to_bytes()];
    let resp =
        provider_directory::ProviderLookupResponse::sign(42, vec![remote.clone()], path, 2, &key);
    provider_directory::handle_lookup_response(resp);

    let request = DiscoveryRequest {
        object_size: 512,
        shares: 2,
        region: None,
        max_price_per_block: None,
        min_success_rate_ppm: None,
        limit: 3,
    };

    let providers = handle
        .guard()
        .discover_providers(request)
        .expect("discover providers");
    assert!(
        providers
            .iter()
            .any(|p| p.provider_id == remote.provider_id),
        "remote provider not discovered from lookup response"
    );
}
