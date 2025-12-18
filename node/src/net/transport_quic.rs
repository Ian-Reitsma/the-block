#![cfg(feature = "quic")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

use concurrency::Bytes;
use crypto_suite::signatures::ed25519::SigningKey;
use diagnostics::anyhow::{anyhow, Result};

use transport::{self, ListenerHandle, ProviderKind};

#[cfg(feature = "s2n-quic")]
use transport::CertAdvertisement as S2nCertAdvertisement;
#[cfg(feature = "s2n-quic")]
use transport::S2N_PROVIDER_ID;

#[cfg(feature = "inhouse")]
use transport::{
    inhouse::{self, Certificate as InhouseCertificate, InhouseCertificateStore},
    inhouse_certificate_store, InhouseAdvertisement, INHOUSE_PROVIDER_ID,
};

#[cfg(feature = "inhouse")]
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(feature = "inhouse")]
use diagnostics::tracing;

#[cfg(feature = "inhouse")]
static INHOUSE_STORE_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

#[cfg(feature = "inhouse")]
fn write_override<'a>(lock: &'a RwLock<Option<PathBuf>>) -> RwLockWriteGuard<'a, Option<PathBuf>> {
    match lock.write() {
        Ok(guard) => guard,
        Err(err) => {
            tracing::warn!(target = "net", "inhouse_cert_store_override_poisoned");
            err.into_inner()
        }
    }
}

#[cfg(feature = "inhouse")]
fn read_override<'a>(lock: &'a RwLock<Option<PathBuf>>) -> RwLockReadGuard<'a, Option<PathBuf>> {
    match lock.read() {
        Ok(guard) => guard,
        Err(err) => {
            tracing::warn!(target = "net", "inhouse_cert_store_override_poisoned");
            err.into_inner()
        }
    }
}

use super::{load_net_key, transport_registry};

#[derive(Clone, Debug)]
pub struct CertAdvertisement {
    pub cert: Bytes,
    pub fingerprint: [u8; 32],
    pub previous: Vec<[u8; 32]>,
    pub verifying_key: Option<[u8; 32]>,
    pub issued_at: Option<SystemTime>,
}

#[cfg(feature = "s2n-quic")]
fn from_s2n_advert(advert: S2nCertAdvertisement) -> CertAdvertisement {
    CertAdvertisement {
        cert: advert.cert,
        fingerprint: advert.fingerprint,
        previous: advert.previous,
        verifying_key: None,
        issued_at: None,
    }
}

#[cfg(feature = "inhouse")]
fn from_inhouse_advert(advert: InhouseAdvertisement) -> CertAdvertisement {
    CertAdvertisement {
        cert: advert.certificate.clone(),
        fingerprint: advert.fingerprint,
        previous: Vec::new(),
        verifying_key: Some(advert.verifying_key),
        issued_at: Some(advert.issued_at),
    }
}

enum ActiveProvider {
    #[cfg(feature = "s2n-quic")]
    S2n(transport::S2nAdapter),
    #[cfg(feature = "inhouse")]
    Inhouse {
        adapter: transport::InhouseAdapter,
        store: &'static InhouseCertificateStore,
    },
}

fn provider_registry() -> Result<transport::ProviderRegistry> {
    transport_registry().ok_or_else(|| anyhow!("transport provider not configured"))
}

fn active_provider() -> Result<ActiveProvider> {
    let registry = provider_registry()?;
    match registry.kind() {
        ProviderKind::S2nQuic => {
            #[cfg(feature = "s2n-quic")]
            {
                let adapter = registry
                    .s2n()
                    .ok_or_else(|| anyhow!("s2n transport provider not available"))?;
                Ok(ActiveProvider::S2n(adapter))
            }
            #[cfg(not(feature = "s2n-quic"))]
            {
                let _ = registry;
                Err(anyhow!("s2n transport provider not compiled"))
            }
        }
        ProviderKind::Inhouse => {
            #[cfg(feature = "inhouse")]
            {
                let adapter = registry
                    .inhouse()
                    .ok_or_else(|| anyhow!("inhouse transport provider not available"))?;
                Ok(ActiveProvider::Inhouse {
                    adapter,
                    store: inhouse_store(),
                })
            }
            #[cfg(not(feature = "inhouse"))]
            {
                let _ = registry;
                Err(anyhow!("inhouse transport provider not compiled"))
            }
        }
        ProviderKind::Quinn => Err(anyhow!("quinn transport handled by net::quic")),
    }
}

#[cfg(feature = "inhouse")]
pub(crate) fn set_inhouse_cert_store_override(path: Option<PathBuf>) {
    let lock = INHOUSE_STORE_OVERRIDE.get_or_init(|| RwLock::new(None));
    *write_override(lock) = path;
}

#[cfg(feature = "inhouse")]
fn inhouse_store() -> &'static InhouseCertificateStore {
    static STORE: OnceLock<InhouseCertificateStore> = OnceLock::new();
    STORE.get_or_init(|| inhouse_certificate_store(cert_store_path()))
}

#[cfg(feature = "inhouse")]
fn cert_store_path() -> PathBuf {
    const CERT_STORE_FILE: &str = "quic_certs.json";
    #[cfg(feature = "inhouse")]
    if let Some(lock) = INHOUSE_STORE_OVERRIDE.get() {
        if let Some(path) = read_override(lock).as_ref().cloned() {
            return path;
        }
    }
    if let Ok(path) = std::env::var("TB_NET_CERT_STORE_PATH") {
        PathBuf::from(path)
    } else {
        sys::paths::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".the_block")
            .join(CERT_STORE_FILE)
    }
}

#[cfg(feature = "inhouse")]
fn certificate_from_store(store: &InhouseCertificateStore) -> Result<InhouseCertificate> {
    // Try to load existing certificate
    if let Some(cert) = store.load_certificate() {
        return Ok(cert);
    }

    // If no certificate exists, generate a new one and install it
    let cert = inhouse::Certificate::generate()
        .map_err(|err| anyhow!("generate inhouse certificate: {err}"))?;
    store
        .install_certificate(&cert)
        .map_err(|err| anyhow!("install inhouse certificate: {err}"))?;
    Ok(cert)
}

pub fn initialize(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    match active_provider()? {
        #[cfg(feature = "s2n-quic")]
        ActiveProvider::S2n(adapter) => adapter
            .initialize(signing_key)
            .map(from_s2n_advert)
            .map_err(Into::into),
        #[cfg(feature = "inhouse")]
        ActiveProvider::Inhouse { store, .. } => {
            let cert = inhouse::Certificate::generate()
                .map_err(|err| anyhow!("generate inhouse certificate: {err}"))?;
            let advert = store
                .install_certificate(&cert)
                .map_err(|err| anyhow!("install inhouse certificate: {err}"))?;
            Ok(from_inhouse_advert(advert))
        }
    }
}

pub fn rotate(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    match active_provider()? {
        #[cfg(feature = "s2n-quic")]
        ActiveProvider::S2n(adapter) => adapter
            .rotate(signing_key)
            .map(from_s2n_advert)
            .map_err(Into::into),
        #[cfg(feature = "inhouse")]
        ActiveProvider::Inhouse { store, .. } => {
            let cert = inhouse::Certificate::generate()
                .map_err(|err| anyhow!("generate inhouse certificate: {err}"))?;
            let advert = store
                .install_certificate(&cert)
                .map_err(|err| anyhow!("rotate inhouse certificate: {err}"))?;
            Ok(from_inhouse_advert(advert))
        }
    }
}

#[cfg(feature = "s2n-quic")]
pub fn current_cert() -> Option<transport::LocalCert> {
    match active_provider() {
        Ok(ActiveProvider::S2n(adapter)) => adapter.current_cert(),
        _ => None,
    }
}

pub fn current_advertisement() -> Option<CertAdvertisement> {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.current_advertisement().map(from_s2n_advert),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { store, .. }) => store.load_certificate().map(|cert| {
            let advert = transport::InhouseAdvertisement::from(&cert);
            from_inhouse_advert(advert)
        }),
        Err(_) => None,
    }
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.fingerprint_history(),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { .. }) => transport::inhouse_fingerprint_history(),
        Err(_) => transport::fingerprint_history(),
    }
}

pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.fingerprint(cert),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { .. }) => transport::inhouse_fingerprint(cert),
        Err(_) => transport::fingerprint(cert),
    }
}

pub fn verify_remote_certificate(peer_key: &[u8; 32], cert: &[u8]) -> Result<[u8; 32]> {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.verify_remote_certificate(peer_key, cert),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { adapter, .. }) => {
            adapter.verify_remote_certificate(peer_key, cert)
        }
        Err(_) => transport::verify_remote_certificate(peer_key, cert),
    }
}

pub async fn start_server(addr: SocketAddr) -> Result<ListenerHandle> {
    match active_provider()? {
        #[cfg(feature = "s2n-quic")]
        ActiveProvider::S2n(adapter) => {
            let key = load_net_key();
            adapter.start_server(addr, &key).await
        }
        #[cfg(feature = "inhouse")]
        ActiveProvider::Inhouse { adapter, store } => {
            let (endpoint, cert_handle) = adapter.listen(addr).await?;
            // Extract the certificate from the handle and store it
            if let transport::CertificateHandle::Inhouse(cert) = &cert_handle {
                // Store the certificate DER bytes
                store
                    .install_certificate(cert)
                    .map_err(|err| anyhow!("persist inhouse certificate: {err}"))?;
            }
            Ok(endpoint)
        }
    }
}

pub async fn connect(addr: SocketAddr) -> Result<()> {
    match active_provider()? {
        #[cfg(feature = "s2n-quic")]
        ActiveProvider::S2n(adapter) => adapter.connect(addr).await,
        #[cfg(feature = "inhouse")]
        ActiveProvider::Inhouse { adapter, store } => {
            let certificate = certificate_from_store(store)?;
            let cert_handle = transport::CertificateHandle::Inhouse(certificate);
            adapter.connect(addr, &cert_handle).await.map(|_| ())
        }
    }
}

pub fn record_handshake_fail(reason: &str) {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.record_handshake_fail(reason),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { .. }) => {
            let _ = reason;
        }
        Err(_) => {}
    }
}

pub fn record_retransmit(count: u64) {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(adapter)) => adapter.record_retransmit(count),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { .. }) => {
            let _ = count;
        }
        Err(_) => {}
    }
}

pub fn provider_id() -> Option<&'static str> {
    match active_provider() {
        #[cfg(feature = "s2n-quic")]
        Ok(ActiveProvider::S2n(_)) => Some(S2N_PROVIDER_ID),
        #[cfg(feature = "inhouse")]
        Ok(ActiveProvider::Inhouse { .. }) => Some(INHOUSE_PROVIDER_ID),
        Err(_) => None,
    }
}
