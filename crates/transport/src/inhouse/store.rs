use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::result::Result as StdResult;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64_fp::{decode_standard, encode_standard};
use concurrency::Bytes;
use diagnostics::{anyhow, Result as DiagResult, TbError};
use foundation_serialization::json::{self, Map, Value};

use super::certificate::Certificate;

#[derive(Clone)]
pub struct Advertisement {
    pub fingerprint: [u8; 32],
    pub verifying_key: [u8; 32],
    pub certificate: Bytes,
    pub issued_at: SystemTime,
}

impl Advertisement {
    fn to_value(&self) -> DiagResult<Value> {
        let mut map = Map::new();
        map.insert(
            "fingerprint".to_string(),
            Value::Array(self.fingerprint.iter().copied().map(Value::from).collect()),
        );
        map.insert(
            "verifying_key".to_string(),
            Value::Array(
                self.verifying_key
                    .iter()
                    .copied()
                    .map(Value::from)
                    .collect(),
            ),
        );
        map.insert(
            "certificate".to_string(),
            Value::String(encode_standard(self.certificate.as_ref())),
        );
        map.insert(
            "issued_at".to_string(),
            Value::Object(system_time_to_map(self.issued_at)?),
        );
        Ok(Value::Object(map))
    }

    fn from_value(value: Value) -> DiagResult<Self> {
        let mut object = match value {
            Value::Object(map) => map,
            other => {
                return Err(anyhow!(
                    "advertisement must be a JSON object, found {}",
                    describe_json(&other)
                ))
            }
        };
        let fingerprint_value = object
            .remove("fingerprint")
            .ok_or_else(|| anyhow!("advertisement missing fingerprint"))?;
        let verifying_key_value = object.remove("verifying_key");
        let certificate_value = object
            .remove("certificate")
            .ok_or_else(|| anyhow!("advertisement missing certificate"))?;
        let issued_at_value = object
            .remove("issued_at")
            .ok_or_else(|| anyhow!("advertisement missing issued_at"))?;
        Ok(Advertisement {
            fingerprint: parse_byte_array(fingerprint_value, "fingerprint")?,
            verifying_key: match verifying_key_value {
                Some(value) => parse_byte_array(value, "verifying_key")?,
                None => [0u8; 32],
            },
            certificate: Bytes::from(parse_certificate(certificate_value)?),
            issued_at: parse_system_time(issued_at_value)?,
        })
    }
}

impl From<&Certificate> for Advertisement {
    fn from(cert: &Certificate) -> Self {
        Self {
            fingerprint: cert.fingerprint,
            verifying_key: cert.verifying_key,
            certificate: cert.der.clone(),
            issued_at: SystemTime::now(),
        }
    }
}

fn describe_json(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parse_byte_array(value: Value, field: &str) -> DiagResult<[u8; 32]> {
    let values = match value {
        Value::Array(values) => values,
        other => {
            return Err(anyhow!(
                "advertisement {} must be an array, found {}",
                field,
                describe_json(&other)
            ))
        }
    };
    if values.len() != 32 {
        return Err(anyhow!(
            "advertisement {} must contain 32 entries, found {}",
            field,
            values.len()
        ));
    }
    let mut out = [0u8; 32];
    for (idx, value) in values.into_iter().enumerate() {
        out[idx] = parse_byte(value, field)?;
    }
    Ok(out)
}

fn parse_byte(value: Value, field: &str) -> DiagResult<u8> {
    let number = match value {
        Value::Number(n) => n,
        Value::String(s) => {
            let parsed: u64 = s
                .parse()
                .map_err(|err| anyhow!("invalid {} byte: {err}", field))?;
            return byte_from_u64(parsed, field);
        }
        other => {
            return Err(anyhow!(
                "{} entries must be numbers, found {}",
                field,
                describe_json(&other)
            ))
        }
    };
    let value = number
        .as_u64()
        .ok_or_else(|| anyhow!("{} entries must be unsigned integers", field))?;
    byte_from_u64(value, field)
}

fn parse_certificate(value: Value) -> DiagResult<Vec<u8>> {
    match value {
        Value::String(encoded) => {
            decode_standard(&encoded).map_err(|err| anyhow!("invalid certificate payload: {err}"))
        }
        Value::Array(values) => {
            let mut out = Vec::with_capacity(values.len());
            for entry in values {
                out.push(parse_byte(entry, "certificate")?);
            }
            Ok(out)
        }
        other => Err(anyhow!(
            "certificate must be encoded as a base64 string or byte array, found {}",
            describe_json(&other)
        )),
    }
}

fn byte_from_u64(value: u64, field: &str) -> DiagResult<u8> {
    if value > u8::MAX as u64 {
        return Err(anyhow!("{} entries must fit in a byte", field));
    }
    Ok(value as u8)
}

fn system_time_to_map(time: SystemTime) -> DiagResult<Map> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("timestamp predates unix epoch"))?;
    let mut map = Map::new();
    map.insert(
        "secs_since_epoch".to_string(),
        Value::from(duration.as_secs()),
    );
    map.insert(
        "nanos_since_epoch".to_string(),
        Value::from(duration.subsec_nanos()),
    );
    Ok(map)
}

fn parse_system_time(value: Value) -> DiagResult<SystemTime> {
    let mut object = match value {
        Value::Object(map) => map,
        other => {
            return Err(anyhow!(
                "issued_at must be an object, found {}",
                describe_json(&other)
            ))
        }
    };
    let secs = parse_u64_field(
        object
            .remove("secs_since_epoch")
            .ok_or_else(|| anyhow!("issued_at missing secs_since_epoch"))?,
        "secs_since_epoch",
    )?;
    let nanos = parse_u64_field(
        object
            .remove("nanos_since_epoch")
            .ok_or_else(|| anyhow!("issued_at missing nanos_since_epoch"))?,
        "nanos_since_epoch",
    )?;
    if nanos >= 1_000_000_000 {
        return Err(anyhow!(
            "nanos_since_epoch must be less than 1_000_000_000, found {nanos}"
        ));
    }
    Ok(UNIX_EPOCH + std::time::Duration::new(secs, nanos as u32))
}

fn parse_u64_field(value: Value, field: &str) -> DiagResult<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| anyhow!("{field} must be an unsigned integer")),
        Value::String(s) => s.parse().map_err(|err| anyhow!("invalid {field}: {err}")),
        other => Err(anyhow!(
            "{field} must be a number or string, found {}",
            describe_json(&other)
        )),
    }
}

#[derive(Clone)]
pub struct InhouseCertificateStore {
    path: PathBuf,
    current: Arc<Mutex<Option<Advertisement>>>,
}

impl InhouseCertificateStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            current: Arc::new(Mutex::new(None)),
        }
    }

    fn persist(&self, advert: &Advertisement) -> DiagResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| anyhow!("create cert dir: {err}"))?;
        }
        let mut file =
            File::create(&self.path).map_err(|err| anyhow!("create cert store: {err}"))?;
        let json_value = advert.to_value()?;
        let json = json::to_vec_value(&json_value);
        file.write_all(&json)
            .map_err(|err| anyhow!("write cert store: {err}"))?;
        file.sync_all()
            .map_err(|err| anyhow!("sync cert store: {err}"))?;
        Ok(())
    }

    fn load(&self) -> DiagResult<Option<Advertisement>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let mut file = File::open(&self.path).map_err(|err| anyhow!("open cert store: {err}"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|err| anyhow!("read cert store: {err}"))?;
        if buf.is_empty() {
            return Ok(None);
        }
        let value =
            json::value_from_slice(&buf).map_err(|err| anyhow!("decode cert store: {err}"))?;
        let advert = Advertisement::from_value(value)?;
        Ok(Some(advert))
    }

    fn generate(&self) -> DiagResult<(Certificate, Advertisement)> {
        let certificate = Certificate::generate()?;
        let advert = Advertisement::from(&certificate);
        Ok((certificate, advert))
    }

    fn regenerate(&self) -> Option<Advertisement> {
        match self.generate().and_then(|(certificate, advert)| {
            self.persist(&advert)?;
            self.write_der(&certificate)?;
            Ok(advert)
        }) {
            Ok(advert) => {
                *self.current.lock().unwrap() = Some(advert.clone());
                Some(advert)
            }
            Err(_) => None,
        }
    }

    fn write_der(&self, certificate: &Certificate) -> DiagResult<()> {
        let path = self.path.with_extension("der");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| anyhow!("create cert dir: {err}"))?;
        }
        let mut file = File::create(&path).map_err(|err| anyhow!("create cert der: {err}"))?;
        file.write_all(certificate.der.as_ref())
            .map_err(|err| anyhow!("write cert der: {err}"))?;
        file.sync_all()
            .map_err(|err| anyhow!("sync cert der: {err}"))?;
        Ok(())
    }

    pub fn install_certificate(&self, certificate: &Certificate) -> DiagResult<Advertisement> {
        let advert = Advertisement::from(certificate);
        self.persist(&advert)?;
        self.write_der(certificate)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    pub fn load_certificate(&self) -> Option<Certificate> {
        let path = self.path.with_extension("der");
        let mut buf = Vec::new();
        let mut file = File::open(&path).ok()?;
        match file.read_to_end(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let certificate = Certificate::from_der_lossy(Bytes::from(buf));
        if certificate.verifying_key == [0u8; 32] {
            let _ = fs::remove_file(path);
            return None;
        }
        Some(certificate)
    }
}

impl crate::CertificateStore for InhouseCertificateStore {
    type Advertisement = Advertisement;
    type Error = TbError;

    fn initialize(&self) -> StdResult<Self::Advertisement, Self::Error> {
        let (certificate, advert) = self.generate()?;
        self.persist(&advert)?;
        self.write_der(&certificate)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    fn rotate(&self) -> StdResult<Self::Advertisement, Self::Error> {
        let (certificate, advert) = self.generate()?;
        self.persist(&advert)?;
        self.write_der(&certificate)?;
        *self.current.lock().unwrap() = Some(advert.clone());
        Ok(advert)
    }

    fn current(&self) -> Option<Self::Advertisement> {
        if let Some(current) = self.current.lock().unwrap().clone() {
            return Some(current);
        }
        match self.load() {
            Ok(Some(advert)) => {
                if advert.verifying_key == [0u8; 32] || advert.certificate.is_empty() {
                    if let Some(fresh) = self.regenerate() {
                        return Some(fresh);
                    }
                }
                *self.current.lock().unwrap() = Some(advert.clone());
                Some(advert)
            }
            Ok(None) => None,
            Err(_) => self.regenerate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_round_trips() {
        let advert = Advertisement {
            fingerprint: [7u8; 32],
            verifying_key: [8u8; 32],
            certificate: Bytes::from_static(b"cert-bytes"),
            issued_at: UNIX_EPOCH + std::time::Duration::new(5, 9),
        };
        let value = advert.to_value().expect("serialize");
        let restored = Advertisement::from_value(value).expect("deserialize");
        assert_eq!(restored.fingerprint, [7u8; 32]);
        assert_eq!(restored.verifying_key, [8u8; 32]);
        assert_eq!(restored.certificate.as_ref(), b"cert-bytes");
        assert_eq!(restored.issued_at, advert.issued_at);
    }

    #[test]
    fn system_time_serialization_matches() {
        let advert = Advertisement {
            fingerprint: [0u8; 32],
            verifying_key: [1u8; 32],
            certificate: Bytes::from_static(b"certificate"),
            issued_at: UNIX_EPOCH + std::time::Duration::new(5, 9),
        };
        let value = advert.to_value().expect("serialize");
        let map = match value {
            Value::Object(map) => map,
            _ => panic!("expected object"),
        };
        let fingerprint = map.get("fingerprint").expect("fingerprint");
        let array = match fingerprint {
            Value::Array(values) => values,
            _ => panic!("fingerprint not array"),
        };
        assert_eq!(array.len(), 32);
        assert_eq!(array[0], Value::from(0u8));
        let verifying_key = map.get("verifying_key").expect("verifying_key");
        let vk_array = match verifying_key {
            Value::Array(values) => values,
            _ => panic!("verifying_key not array"),
        };
        assert_eq!(vk_array.len(), 32);
        assert_eq!(vk_array[0], Value::from(1u8));
        let issued_at = map.get("issued_at").expect("issued_at");
        let issued_map = match issued_at {
            Value::Object(map) => map,
            _ => panic!("issued_at not object"),
        };
        assert_eq!(issued_map.get("secs_since_epoch"), Some(&Value::from(5u64)));
        assert_eq!(
            issued_map.get("nanos_since_epoch"),
            Some(&Value::from(9u32))
        );
        let cert_value = map.get("certificate").expect("certificate");
        match cert_value {
            Value::String(encoded) => {
                let decoded = decode_standard(encoded).expect("decode certificate");
                assert_eq!(decoded, b"certificate");
            }
            other => panic!("certificate not encoded as string: {other:?}"),
        }
    }

    #[test]
    fn install_certificate_persists_der() {
        let temp = sys::tempfile::TempDir::new().expect("temp dir");
        let store_path = temp.path().join("cert.json");
        let store = InhouseCertificateStore::new(store_path.clone());
        let certificate = Certificate::generate().expect("generate cert");
        let advert = store
            .install_certificate(&certificate)
            .expect("install certificate");
        assert_eq!(advert.fingerprint, certificate.fingerprint);

        let loaded = store.load_certificate().expect("load certificate");
        assert_eq!(loaded.fingerprint, certificate.fingerprint);

        let fresh_store = InhouseCertificateStore::new(store_path);
        let recovered = fresh_store.load_certificate().expect("recover certificate");
        assert_eq!(recovered.fingerprint, certificate.fingerprint);
    }

    #[test]
    fn load_certificate_rejects_corrupt_der() {
        let temp = sys::tempfile::TempDir::new().expect("temp dir");
        let store_path = temp.path().join("cert.json");
        let store = InhouseCertificateStore::new(store_path.clone());
        let der_path = store_path.with_extension("der");
        if let Some(parent) = der_path.parent() {
            fs::create_dir_all(parent).expect("create dir");
        }
        fs::write(&der_path, b"invalid-der").expect("write corrupt der");
        assert!(store.load_certificate().is_none());
        assert!(!der_path.exists());
    }
}
