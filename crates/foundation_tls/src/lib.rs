#![forbid(unsafe_code)]

use std::borrow::Cow;

use crypto_suite::{
    hashing::blake3,
    signatures::ed25519::{SigningKey, VerifyingKey},
};
use foundation_time::{Duration, UtcDateTime};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("certificate subject is required")]
    MissingSubject,
    #[error("certificate validity window is required")]
    MissingValidity,
    #[error("certificate validity window is inverted")]
    InvalidValidity,
    #[error("rotation schedule is invalid")]
    InvalidSchedule,
    #[error("rotation schedule produced an out-of-range timestamp")]
    ScheduleOutOfRange,
    #[error("certificate serial number is required")]
    MissingSerial,
    #[error("subject alt name must be ASCII")]
    InvalidSubjectAltName,
    #[error("timestamp could not be encoded")]
    TimestampEncoding,
    #[error("key encoding failed")]
    KeyEncoding,
}

#[derive(Clone, Debug)]
pub enum SubjectAltName<'a> {
    Dns(Cow<'a, str>),
}

#[derive(Clone, Debug)]
pub struct SelfSignedCertParams<'a> {
    subject_cn: Cow<'a, str>,
    alt_names: Vec<SubjectAltName<'a>>,
    not_before: UtcDateTime,
    not_after: UtcDateTime,
    serial: [u8; 16],
    is_ca: bool,
}

impl<'a> SelfSignedCertParams<'a> {
    pub fn builder() -> SelfSignedCertParamsBuilder<'a> {
        SelfSignedCertParamsBuilder::default()
    }

    pub fn subject_cn(&self) -> &str {
        &self.subject_cn
    }

    pub fn alt_names(&self) -> &[SubjectAltName<'a>] {
        &self.alt_names
    }

    pub fn not_before(&self) -> UtcDateTime {
        self.not_before
    }

    pub fn not_after(&self) -> UtcDateTime {
        self.not_after
    }

    pub fn serial(&self) -> &[u8; 16] {
        &self.serial
    }

    pub fn is_ca(&self) -> bool {
        self.is_ca
    }
}

#[derive(Default)]
pub struct SelfSignedCertParamsBuilder<'a> {
    subject_cn: Option<Cow<'a, str>>,
    alt_names: Vec<SubjectAltName<'a>>,
    not_before: Option<UtcDateTime>,
    not_after: Option<UtcDateTime>,
    serial: Option<[u8; 16]>,
    is_ca: bool,
}

impl<'a> SelfSignedCertParamsBuilder<'a> {
    pub fn subject_cn(mut self, value: impl Into<Cow<'a, str>>) -> Self {
        self.subject_cn = Some(value.into());
        self
    }

    pub fn add_dns_name(mut self, value: impl Into<Cow<'a, str>>) -> Self {
        self.alt_names.push(SubjectAltName::Dns(value.into()));
        self
    }

    pub fn validity(mut self, not_before: UtcDateTime, not_after: UtcDateTime) -> Self {
        self.not_before = Some(not_before);
        self.not_after = Some(not_after);
        self
    }

    pub fn serial(mut self, serial: [u8; 16]) -> Self {
        self.serial = Some(serial);
        self
    }

    pub fn apply_rotation_plan(mut self, plan: &RotationPlan) -> Self {
        self.not_before = Some(plan.not_before());
        self.not_after = Some(plan.not_after());
        self.serial = Some(*plan.serial());
        self
    }

    pub fn ca(mut self, is_ca: bool) -> Self {
        self.is_ca = is_ca;
        self
    }

    pub fn build(self) -> Result<SelfSignedCertParams<'a>, CertificateError> {
        let subject_cn = self.subject_cn.ok_or(CertificateError::MissingSubject)?;
        let not_before = self.not_before.ok_or(CertificateError::MissingValidity)?;
        let not_after = self.not_after.ok_or(CertificateError::MissingValidity)?;
        if not_after <= not_before {
            return Err(CertificateError::InvalidValidity);
        }
        let serial = self.serial.ok_or(CertificateError::MissingSerial)?;
        Ok(SelfSignedCertParams {
            subject_cn,
            alt_names: self.alt_names,
            not_before,
            not_after,
            serial,
            is_ca: self.is_ca,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RotationPolicy {
    anchor: UtcDateTime,
    period: Duration,
    overlap: Duration,
}

impl RotationPolicy {
    pub fn new(
        anchor: UtcDateTime,
        period: Duration,
        overlap: Duration,
    ) -> Result<Self, CertificateError> {
        let period_secs = period.total_seconds();
        let overlap_secs = overlap.total_seconds();
        if period_secs <= 0 || overlap_secs < 0 || overlap_secs >= period_secs {
            return Err(CertificateError::InvalidSchedule);
        }
        Ok(Self {
            anchor,
            period,
            overlap,
        })
    }

    pub fn slot_at(&self, moment: UtcDateTime) -> Result<u64, CertificateError> {
        let anchor_secs = timestamp_i128(self.anchor)?;
        let moment_secs = timestamp_i128(moment)?;
        let period_secs = self.period.total_seconds();
        if period_secs <= 0 {
            return Err(CertificateError::InvalidSchedule);
        }
        let delta = moment_secs - anchor_secs;
        if delta <= 0 {
            return Ok(0);
        }
        Ok((delta / period_secs) as u64)
    }

    pub fn plan(&self, slot: u64, context: &[u8]) -> Result<RotationPlan, CertificateError> {
        let period_secs = self.period.total_seconds();
        let overlap_secs = self.overlap.total_seconds();
        if period_secs <= 0 || overlap_secs < 0 || overlap_secs >= period_secs {
            return Err(CertificateError::InvalidSchedule);
        }
        let anchor_secs = timestamp_i128(self.anchor)?;
        let slot_offset = (slot as i128)
            .checked_mul(period_secs)
            .ok_or(CertificateError::ScheduleOutOfRange)?;
        let start_secs = anchor_secs
            .checked_add(slot_offset)
            .and_then(|v| v.checked_sub(overlap_secs))
            .ok_or(CertificateError::ScheduleOutOfRange)?;
        let end_secs = anchor_secs
            .checked_add(slot_offset)
            .and_then(|v| v.checked_add(period_secs))
            .and_then(|v| v.checked_add(overlap_secs))
            .ok_or(CertificateError::ScheduleOutOfRange)?;
        let not_before = secs_to_datetime(start_secs)?;
        let not_after = secs_to_datetime(end_secs)?;
        let serial = derive_serial(self.anchor, self.period, self.overlap, slot, context)?;
        Ok(RotationPlan {
            not_before,
            not_after,
            serial,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RotationPlan {
    not_before: UtcDateTime,
    not_after: UtcDateTime,
    serial: [u8; 16],
}

impl RotationPlan {
    pub fn not_before(&self) -> UtcDateTime {
        self.not_before
    }

    pub fn not_after(&self) -> UtcDateTime {
        self.not_after
    }

    pub fn serial(&self) -> &[u8; 16] {
        &self.serial
    }
}

#[derive(Clone, Debug)]
pub struct GeneratedEd25519Cert {
    pub certificate: Vec<u8>,
    pub private_key: Vec<u8>,
    pub public_key: [u8; 32],
}

pub fn generate_self_signed_ed25519(
    params: &SelfSignedCertParams<'_>,
) -> Result<GeneratedEd25519Cert, CertificateError> {
    let mut rng = OsRng::default();
    let signing_key = SigningKey::generate(&mut rng);
    let certificate = sign_self_signed_ed25519(&signing_key, params)?;
    let private_key = signing_key
        .to_pkcs8_der()
        .map_err(|_| CertificateError::KeyEncoding)?
        .as_bytes()
        .to_vec();
    let public_key = signing_key.verifying_key().to_bytes();
    Ok(GeneratedEd25519Cert {
        certificate,
        private_key,
        public_key,
    })
}

pub fn sign_self_signed_ed25519(
    signing_key: &SigningKey,
    params: &SelfSignedCertParams<'_>,
) -> Result<Vec<u8>, CertificateError> {
    let public_key = signing_key.verifying_key();
    let tbs = build_tbs_certificate(&public_key, params, params.subject_cn())?;
    let signature_bytes = signing_key.sign(&tbs).to_bytes();
    Ok(assemble_certificate(tbs, &signature_bytes))
}

pub fn sign_with_ca_ed25519(
    issuer_key: &SigningKey,
    issuer_cn: &str,
    subject_key: &SigningKey,
    params: &SelfSignedCertParams<'_>,
) -> Result<Vec<u8>, CertificateError> {
    let tbs = build_tbs_certificate(&subject_key.verifying_key(), params, issuer_cn)?;
    let signature_bytes = issuer_key.sign(&tbs).to_bytes();
    Ok(assemble_certificate(tbs, &signature_bytes))
}

fn build_tbs_certificate(
    public_key: &VerifyingKey,
    params: &SelfSignedCertParams<'_>,
    issuer_cn: &str,
) -> Result<Vec<u8>, CertificateError> {
    let version = encode_explicit(0, &encode_integer(&[2]));
    let serial = encode_integer(params.serial());
    let algorithm = algorithm_identifier();
    let issuer = encode_name(issuer_cn);
    let validity = encode_validity(params)?;
    let subject = encode_name(params.subject_cn());
    let spki = encode_subject_public_key(public_key);
    let mut parts = vec![
        version,
        serial,
        algorithm.clone(),
        issuer,
        validity,
        subject,
        spki,
    ];
    if let Some(extensions) = encode_extensions(params)? {
        parts.push(extensions);
    }
    Ok(sequence(parts))
}

fn assemble_certificate(tbs: Vec<u8>, signature: &[u8; 64]) -> Vec<u8> {
    let algorithm = algorithm_identifier();
    let mut sig_body = Vec::with_capacity(1 + signature.len());
    sig_body.push(0u8);
    sig_body.extend_from_slice(signature);
    sequence(vec![tbs, algorithm, wrap(0x03, &sig_body)])
}

fn encode_extensions(
    params: &SelfSignedCertParams<'_>,
) -> Result<Option<Vec<u8>>, CertificateError> {
    let mut extensions = Vec::new();
    if params.is_ca() {
        extensions.push(encode_basic_constraints());
    }
    if !params.alt_names().is_empty() {
        extensions.push(encode_subject_alt_name(params)?);
    }
    if extensions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(encode_explicit(3, &sequence(extensions))))
    }
}

fn encode_basic_constraints() -> Vec<u8> {
    let constraints = sequence(vec![encode_boolean(true)]);
    sequence(vec![
        encode_oid(&[2, 5, 29, 19]),
        encode_boolean(true),
        wrap(0x04, &constraints),
    ])
}

fn encode_subject_alt_name(params: &SelfSignedCertParams<'_>) -> Result<Vec<u8>, CertificateError> {
    let mut general_names = Vec::new();
    for name in params.alt_names() {
        match name {
            SubjectAltName::Dns(value) => {
                if !value.is_ascii() {
                    return Err(CertificateError::InvalidSubjectAltName);
                }
                general_names.push(encode_context_ia5(2, value.as_bytes()));
            }
        }
    }
    let general_names_seq = wrap(0x30, &concat(&general_names));
    let ext_value = wrap(0x04, &general_names_seq);
    Ok(sequence(vec![encode_oid(&[2, 5, 29, 17]), ext_value]))
}

fn encode_subject_public_key(public_key: &VerifyingKey) -> Vec<u8> {
    let mut bit_string = Vec::with_capacity(1 + public_key.to_bytes().len());
    bit_string.push(0);
    bit_string.extend_from_slice(&public_key.to_bytes());
    sequence(vec![algorithm_identifier(), wrap(0x03, &bit_string)])
}

fn encode_validity(params: &SelfSignedCertParams<'_>) -> Result<Vec<u8>, CertificateError> {
    let not_before = encode_generalized_time(params.not_before())?;
    let not_after = encode_generalized_time(params.not_after())?;
    Ok(sequence(vec![not_before, not_after]))
}

fn encode_generalized_time(time: UtcDateTime) -> Result<Vec<u8>, CertificateError> {
    let comps = time
        .components()
        .map_err(|_| CertificateError::TimestampEncoding)?;
    let mut buf = [0u8; 15];
    write_four(&mut buf[0..4], comps.year as i32);
    write_two(&mut buf[4..6], comps.month as u8);
    write_two(&mut buf[6..8], comps.day as u8);
    write_two(&mut buf[8..10], comps.hour);
    write_two(&mut buf[10..12], comps.minute);
    write_two(&mut buf[12..14], comps.second);
    buf[14] = b'Z';
    Ok(wrap(0x18, &buf))
}

fn write_four(dst: &mut [u8], value: i32) {
    let digits = format!("{value:04}");
    dst.copy_from_slice(digits.as_bytes());
}

fn write_two(dst: &mut [u8], value: u8) {
    dst.copy_from_slice(format!("{value:02}").as_bytes());
}

fn encode_name(value: &str) -> Vec<u8> {
    let cn = sequence(vec![encode_oid(&[2, 5, 4, 3]), encode_utf8(value)]);
    sequence(vec![set(vec![cn])])
}

fn algorithm_identifier() -> Vec<u8> {
    sequence(vec![encode_oid(&[1, 3, 101, 112])])
}

fn encode_utf8(value: &str) -> Vec<u8> {
    wrap(0x0C, value.as_bytes())
}

fn encode_boolean(value: bool) -> Vec<u8> {
    wrap(0x01, &[if value { 0xFF } else { 0x00 }])
}

fn encode_context_ia5(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + 4);
    out.push(0x80 | tag);
    encode_length(value.len(), &mut out);
    out.extend_from_slice(value);
    out
}

fn encode_integer(bytes: &[u8]) -> Vec<u8> {
    let mut start = 0;
    while start + 1 < bytes.len() && bytes[start] == 0 {
        start += 1;
    }
    let mut value = bytes[start..].to_vec();
    if value.first().map(|b| b & 0x80 != 0).unwrap_or(false) {
        value.insert(0, 0);
    }
    wrap(0x02, &value)
}

fn encode_oid(oid: &[u32]) -> Vec<u8> {
    let mut content = Vec::new();
    if oid.len() >= 2 {
        content.push((oid[0] * 40 + oid[1]) as u8);
    }
    for &part in &oid[2..] {
        encode_base128(part, &mut content);
    }
    wrap(0x06, &content)
}

fn encode_base128(mut value: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 5];
    let mut idx = buf.len();
    loop {
        idx -= 1;
        buf[idx] = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            break;
        }
    }
    let mut first = true;
    for byte in &buf[idx..] {
        if first {
            out.push(*byte);
            first = false;
        } else {
            out.push(*byte | 0x80);
        }
    }
}

fn encode_explicit(tag: u8, inner: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xA0 | tag);
    encode_length(inner.len(), &mut out);
    out.extend_from_slice(inner);
    out
}

fn sequence(parts: Vec<Vec<u8>>) -> Vec<u8> {
    wrap(0x30, &concat(&parts))
}

fn set(parts: Vec<Vec<u8>>) -> Vec<u8> {
    wrap(0x31, &concat(&parts))
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    let total = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(total);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

fn wrap(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 5);
    out.push(tag);
    encode_length(content.len(), &mut out);
    out.extend_from_slice(content);
    out
}

fn encode_length(len: usize, out: &mut Vec<u8>) {
    if len < 0x80 {
        out.push(len as u8);
        return;
    }
    let mut buf = [0u8; 8];
    let mut value = len;
    let mut idx = buf.len();
    while value > 0 {
        idx -= 1;
        buf[idx] = (value & 0xFF) as u8;
        value >>= 8;
    }
    let bytes = &buf[idx..];
    out.push(0x80 | bytes.len() as u8);
    out.extend_from_slice(bytes);
}

fn timestamp_i128(time: UtcDateTime) -> Result<i128, CertificateError> {
    let seconds = time
        .unix_timestamp()
        .map_err(|_| CertificateError::ScheduleOutOfRange)?;
    Ok(seconds as i128)
}

fn secs_to_datetime(secs: i128) -> Result<UtcDateTime, CertificateError> {
    if secs < i64::MIN as i128 || secs > i64::MAX as i128 {
        return Err(CertificateError::ScheduleOutOfRange);
    }
    UtcDateTime::from_unix_timestamp(secs as i64).map_err(|_| CertificateError::ScheduleOutOfRange)
}

fn derive_serial(
    anchor: UtcDateTime,
    period: Duration,
    overlap: Duration,
    slot: u64,
    context: &[u8],
) -> Result<[u8; 16], CertificateError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&timestamp_i128(anchor)?.to_le_bytes());
    hasher.update(&period.total_seconds().to_le_bytes());
    hasher.update(&overlap.total_seconds().to_le_bytes());
    hasher.update(&slot.to_le_bytes());
    hasher.update(context);
    let hash = hasher.finalize();
    let mut serial = [0u8; 16];
    serial.copy_from_slice(&hash.as_bytes()[..16]);
    serial[0] &= 0x7F;
    Ok(serial)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundation_time::{Duration, UtcDateTime};

    #[test]
    fn builds_certificate() {
        let now = UtcDateTime::now();
        let params = SelfSignedCertParams::builder()
            .subject_cn("test-cert")
            .add_dns_name("localhost")
            .validity(now - Duration::hours(1), now + Duration::days(1))
            .serial([1; 16])
            .build()
            .unwrap();
        let mut rng = OsRng::default();
        let signing_key = SigningKey::generate(&mut rng);
        let cert = sign_self_signed_ed25519(&signing_key, &params).unwrap();
        assert!(cert.len() > 100);
    }

    #[test]
    fn ca_signed_certificate() {
        let now = UtcDateTime::now();
        let ca_params = SelfSignedCertParams::builder()
            .subject_cn("test-ca")
            .validity(now - Duration::hours(1), now + Duration::days(7))
            .serial([2; 16])
            .ca(true)
            .build()
            .unwrap();
        let mut rng = OsRng::default();
        let ca_key = SigningKey::generate(&mut rng);
        let _ca_cert = sign_self_signed_ed25519(&ca_key, &ca_params).unwrap();
        let leaf_params = SelfSignedCertParams::builder()
            .subject_cn("leaf")
            .validity(now - Duration::hours(1), now + Duration::days(3))
            .serial([3; 16])
            .build()
            .unwrap();
        let leaf_key = SigningKey::generate(&mut rng);
        let cert =
            sign_with_ca_ed25519(&ca_key, ca_params.subject_cn(), &leaf_key, &leaf_params).unwrap();
        assert!(cert.len() > 100);
    }

    #[test]
    fn rotation_policy_generates_deterministic_serials() {
        let anchor = UtcDateTime::from_unix_timestamp(0).unwrap();
        let policy = RotationPolicy::new(anchor, Duration::days(7), Duration::hours(1)).unwrap();
        let plan_a = policy.plan(5, b"node-a").unwrap();
        let plan_b = policy.plan(5, b"node-a").unwrap();
        assert_eq!(plan_a.serial(), plan_b.serial());
        assert!(plan_a.not_after() > plan_a.not_before());
    }
}
