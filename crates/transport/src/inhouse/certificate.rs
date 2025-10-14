use concurrency::Bytes;
use crypto_suite::{hashing::blake3::hash, signatures::ed25519::SigningKey};
use diagnostics::{anyhow, Result as DiagResult};
use foundation_time::{Duration as TimeDuration, UtcDateTime};
use foundation_tls::{ed25519_public_key_from_der, sign_self_signed_ed25519, SelfSignedCertParams};
use rand::{rngs::OsRng, RngCore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub fingerprint: [u8; 32],
    pub verifying_key: [u8; 32],
    pub der: Bytes,
}

impl Certificate {
    pub fn generate() -> DiagResult<Self> {
        let now = UtcDateTime::now();
        let not_before = now - TimeDuration::hours(1);
        let not_after = now + TimeDuration::days(30);
        let mut serial = [0u8; 16];
        OsRng::default().fill_bytes(&mut serial);
        let params = SelfSignedCertParams::builder()
            .subject_cn("the-block inhouse transport")
            .add_dns_name("the-block")
            .validity(not_before, not_after)
            .serial(serial)
            .build()
            .map_err(|err| anyhow!("build certificate params: {err}"))?;
        let mut rng = OsRng::default();
        let signing_key = SigningKey::generate(&mut rng);
        let certificate = sign_self_signed_ed25519(&signing_key, &params)
            .map_err(|err| anyhow!("sign certificate: {err}"))?;
        let fingerprint = fingerprint(&certificate);
        Ok(Self {
            fingerprint,
            verifying_key: signing_key.verifying_key().to_bytes(),
            der: Bytes::from(certificate),
        })
    }

    pub fn from_der_lossy(der: Bytes) -> Self {
        let fingerprint = fingerprint(der.as_ref());
        let verifying_key = ed25519_public_key_from_der(der.as_ref()).unwrap_or([0u8; 32]);
        Self {
            fingerprint,
            verifying_key,
            der,
        }
    }
}

pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    let digest = hash(cert);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_bytes());
    out
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    Vec::new()
}
