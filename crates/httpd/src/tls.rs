use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use base64_fp as base64;
use crypto_suite::encryption::symmetric::{decrypt_aes256_cbc, encrypt_aes256_cbc};
use crypto_suite::encryption::x25519::{PublicKey as X25519Public, SecretKey as X25519Secret};
use crypto_suite::key_derivation::inhouse::derive_key_material;
use crypto_suite::mac::hmac_sha256;
use crypto_suite::signatures::ed25519::{self, Signature, SigningKey, VerifyingKey};
use diagnostics::debug;
use foundation_serialization::json::{self, Map, Value};
use rand::RngCore;
use rand::rngs::OsRng;
use runtime::net::TcpStream;

pub(crate) const HANDSHAKE_MAGIC: &[u8; 4] = b"TBHS";
pub(crate) const HANDSHAKE_VERSION: u8 = 1;
pub(crate) const HANDSHAKE_MAX_LEN: usize = 8 * 1024;
const SESSION_INFO: &[u8] = b"tb-httpd-session-keys";
pub(crate) const CLIENT_AUTH_INFO: &[u8] = b"tb-httpd-client-auth";
pub(crate) const AES_BLOCK: usize = 16;
pub(crate) const MAC_LEN: usize = 32;

#[derive(Debug, Clone)]
pub struct ServerIdentity {
    signing: Arc<SigningKey>,
    certificate: Arc<Certificate>,
}

#[derive(Debug, Clone)]
pub struct Certificate {
    raw: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ClientRegistry {
    allowed: Arc<HashSet<[u8; ed25519::PUBLIC_KEY_LENGTH]>>,
}

#[derive(Debug, Clone)]
pub enum ClientAuthPolicy {
    None,
    Optional(ClientRegistry),
    Required(ClientRegistry),
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Encoding(String),
    InvalidHandshake(&'static str),
    UnknownClient,
    SignatureFailed,
    Crypto(String),
}

#[derive(Debug, Clone)]
pub struct SessionKeys {
    pub server_write: [u8; 32],
    pub client_write: [u8; 32],
    pub server_mac: [u8; 32],
    pub client_mac: [u8; 32],
}

struct ClientHello {
    magic: [u8; 4],
    version: u8,
    client_ephemeral: [u8; 32],
    client_nonce: [u8; 32],
    certificate: Option<Vec<u8>>,
    signature: Option<Vec<u8>>,
}

struct ServerHello {
    magic: [u8; 4],
    version: u8,
    server_ephemeral: [u8; 32],
    server_nonce: [u8; 32],
    certificate: Vec<u8>,
    signature: Vec<u8>,
    client_auth_required: bool,
}

impl ServerIdentity {
    pub fn from_files(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, Error> {
        let cert_bytes = fs::read(cert_path).map_err(Error::Io)?;
        let verifying = parse_certificate(&cert_bytes)?;
        let key_bytes = fs::read(key_path).map_err(Error::Io)?;
        let signing = parse_signing_key(&key_bytes)?;
        if signing.verifying_key().to_bytes() != verifying.to_bytes() {
            return Err(Error::InvalidHandshake(
                "private key does not match certificate public key",
            ));
        }
        Ok(ServerIdentity {
            signing: Arc::new(signing),
            certificate: Arc::new(Certificate {
                raw: Arc::new(cert_bytes),
            }),
        })
    }

    pub fn certificate_bytes(&self) -> &[u8] {
        &self.certificate.raw
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing
    }
}

impl ClientRegistry {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let bytes = fs::read(path).map_err(Error::Io)?;
        let value: Value =
            json::from_slice(&bytes).map_err(|err| Error::Encoding(err.to_string()))?;
        let map = match &value {
            Value::Object(map) => map,
            _ => return Err(Error::InvalidHandshake("client registry must be an object")),
        };
        let version = expect_u64(map, "version")?;
        if version != 1 {
            return Err(Error::InvalidHandshake("client registry version"));
        }
        let entries = map
            .get("allowed")
            .ok_or_else(|| Error::InvalidHandshake("client registry allowed list"))?;
        let array = match entries {
            Value::Array(values) => values,
            _ => return Err(Error::InvalidHandshake("client registry allowed list")),
        };
        let mut allowed = HashSet::new();
        for entry in array {
            let entry_map = match entry {
                Value::Object(map) => map,
                _ => return Err(Error::InvalidHandshake("client registry entry")),
            };
            let algorithm = expect_string(entry_map, "algorithm")?;
            if !algorithm.eq_ignore_ascii_case(ed25519::ALGORITHM) {
                return Err(Error::InvalidHandshake(
                    "client registry algorithm must be ed25519",
                ));
            }
            let public_key = expect_string(entry_map, "public_key")?;
            let verifying = decode_public_key(&public_key)?;
            allowed.insert(verifying.to_bytes());
        }
        Ok(ClientRegistry {
            allowed: Arc::new(allowed),
        })
    }

    pub fn contains(&self, key: &VerifyingKey) -> bool {
        let key_bytes = key.to_bytes();
        self.allowed.contains(&key_bytes)
    }
}

impl ClientAuthPolicy {
    pub fn requires_client_cert(&self) -> bool {
        matches!(self, ClientAuthPolicy::Required(_))
    }
}

impl SessionKeys {
    pub(crate) fn derive(
        shared: &[u8; 32],
        client_nonce: &[u8; 32],
        server_nonce: &[u8; 32],
    ) -> Result<Self, Error> {
        let mut material =
            Vec::with_capacity(shared.len() + client_nonce.len() + server_nonce.len());
        material.extend_from_slice(shared);
        material.extend_from_slice(client_nonce);
        material.extend_from_slice(server_nonce);
        let mut out = [0u8; 32 * 4];
        derive_key_material(None, SESSION_INFO, &material, &mut out);
        let mut server_write = [0u8; 32];
        let mut client_write = [0u8; 32];
        let mut server_mac = [0u8; 32];
        let mut client_mac = [0u8; 32];
        server_write.copy_from_slice(&out[..32]);
        client_write.copy_from_slice(&out[32..64]);
        server_mac.copy_from_slice(&out[64..96]);
        client_mac.copy_from_slice(&out[96..128]);
        Ok(SessionKeys {
            server_write,
            client_write,
            server_mac,
            client_mac,
        })
    }
}

impl Error {
    pub fn into_io(self) -> io::Error {
        match self {
            Error::Io(err) => err,
            Error::Encoding(err) => io::Error::new(io::ErrorKind::InvalidData, err),
            Error::InvalidHandshake(msg) => io::Error::new(io::ErrorKind::InvalidData, msg),
            Error::UnknownClient => {
                io::Error::new(io::ErrorKind::PermissionDenied, "unknown client")
            }
            Error::SignatureFailed => {
                io::Error::new(io::ErrorKind::PermissionDenied, "invalid signature")
            }
            Error::Crypto(msg) => io::Error::new(io::ErrorKind::InvalidData, msg),
        }
    }
}

pub struct HandshakeOutcome {
    pub session: SessionKeys,
    pub client_key: Option<VerifyingKey>,
}

pub async fn perform_handshake(
    stream: &mut TcpStream,
    identity: &ServerIdentity,
    auth: &ClientAuthPolicy,
) -> io::Result<HandshakeOutcome> {
    let debug = std::env::var("TB_TLS_TEST_DEBUG").is_ok();
    if debug {
        eprintln!("[tls] performing handshake; waiting for client length prefix");
    }
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if debug {
        eprintln!("[tls] accepted connection, waiting for client hello (len={frame_len})");
    }
    if frame_len > HANDSHAKE_MAX_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("handshake frame too large: {frame_len}"),
        ));
    }
    let mut frame = vec![0u8; frame_len];
    stream.read_exact(&mut frame).await?;
    if debug {
        eprintln!("[tls] received client hello frame ({} bytes)", frame.len());
    }
    let client = ClientHello::decode(&frame).map_err(Error::into_io)?;
    if &client.magic != HANDSHAKE_MAGIC {
        return Err(Error::InvalidHandshake("invalid handshake magic").into_io());
    }
    if client.version != HANDSHAKE_VERSION {
        return Err(Error::InvalidHandshake("unsupported handshake version").into_io());
    }
    let client_ephemeral = X25519Public::from_bytes(&client.client_ephemeral)
        .map_err(|_| Error::InvalidHandshake("client ephemeral length").into_io())?;
    let client_nonce = client.client_nonce;
    let client_identity = match auth {
        ClientAuthPolicy::None => None,
        ClientAuthPolicy::Optional(registry) => {
            // Optional auth accepts unauthenticated clients; only verified identities are recorded.
            if let Some(cert_bytes) = &client.certificate {
                let verifying = parse_certificate(cert_bytes).map_err(Error::into_io)?;
                if !registry.contains(&verifying) {
                    debug!("client certificate not in registry; treating as unauthenticated");
                    None
                } else if let Some(signature) = &client.signature {
                    if signature.len() != ed25519::SIGNATURE_LENGTH {
                        debug!("client signature length invalid; treating as unauthenticated");
                        None
                    } else {
                        let mut sig_bytes = [0u8; ed25519::SIGNATURE_LENGTH];
                        sig_bytes.copy_from_slice(signature);
                        let sig = Signature::from_bytes(&sig_bytes);
                        let mut message = Vec::with_capacity(
                            client.client_ephemeral.len() + client.client_nonce.len(),
                        );
                        message.extend_from_slice(&client.client_ephemeral);
                        message.extend_from_slice(&client.client_nonce);
                        if verifying.verify(&message, &sig).is_ok() {
                            Some(verifying)
                        } else {
                            debug!("client signature invalid; treating as unauthenticated");
                            None
                        }
                    }
                } else {
                    debug!("client certificate missing signature; treating as unauthenticated");
                    None
                }
            } else {
                None
            }
        }
        ClientAuthPolicy::Required(registry) => {
            let cert_bytes = client
                .certificate
                .as_ref()
                .ok_or_else(|| Error::InvalidHandshake("missing client certificate").into_io())?;
            let verifying = parse_certificate(cert_bytes).map_err(Error::into_io)?;
            if !registry.contains(&verifying) {
                return Err(Error::UnknownClient.into_io());
            }
            let signature = client
                .signature
                .as_ref()
                .ok_or_else(|| Error::InvalidHandshake("missing client signature").into_io())?;
            if signature.len() != ed25519::SIGNATURE_LENGTH {
                return Err(Error::InvalidHandshake("client signature length").into_io());
            }
            let mut sig_bytes = [0u8; ed25519::SIGNATURE_LENGTH];
            sig_bytes.copy_from_slice(signature);
            let sig = Signature::from_bytes(&sig_bytes);
            let mut message =
                Vec::with_capacity(client.client_ephemeral.len() + client.client_nonce.len());
            message.extend_from_slice(&client.client_ephemeral);
            message.extend_from_slice(&client.client_nonce);
            verifying
                .verify(&message, &sig)
                .map_err(|_| Error::SignatureFailed.into_io())?;
            Some(verifying)
        }
    };
    let mut rng = OsRng::default();
    let server_secret = X25519Secret::generate(&mut rng);
    let server_ephemeral = server_secret.public_key();
    let mut server_nonce = [0u8; 32];
    rng.fill_bytes(&mut server_nonce);
    let shared = server_secret.diffie_hellman(&client_ephemeral).to_bytes();
    let transcript = build_server_transcript(
        &client.client_ephemeral,
        &client.client_nonce,
        &server_ephemeral.to_bytes(),
        &server_nonce,
    );
    let signature = identity.signing_key().sign(&transcript);
    let server_msg = ServerHello {
        magic: *HANDSHAKE_MAGIC,
        version: HANDSHAKE_VERSION,
        server_ephemeral: server_ephemeral.to_bytes(),
        server_nonce,
        certificate: identity.certificate_bytes().to_vec(),
        signature: signature.to_bytes().to_vec(),
        client_auth_required: auth.requires_client_cert(),
    };
    let payload = server_msg.encode();
    let len_buf = (payload.len() as u32).to_be_bytes();
    if debug {
        eprintln!(
            "[tls] writing server hello len_buf ({} bytes)",
            len_buf.len()
        );
    }
    stream.write_all(&len_buf).await?;
    if debug {
        eprintln!(
            "[tls] wrote len_buf, now writing payload ({} bytes)",
            payload.len()
        );
    }
    stream.write_all(&payload).await?;
    if debug {
        eprintln!("[tls] wrote payload, now flushing");
    }
    stream.flush().await?;
    if debug {
        eprintln!(
            "[tls] sent server hello (payload={}, requires_cert={})",
            payload.len(),
            auth.requires_client_cert()
        );
    }
    let session = SessionKeys::derive(&shared, &client_nonce, &server_nonce)?;
    Ok(HandshakeOutcome {
        session,
        client_key: client_identity,
    })
}

pub(crate) fn build_server_transcript(
    client_ephemeral: &[u8; 32],
    client_nonce: &[u8; 32],
    server_ephemeral: &[u8; 32],
    server_nonce: &[u8; 32],
) -> Vec<u8> {
    let mut transcript = Vec::with_capacity(32 * 4);
    transcript.extend_from_slice(CLIENT_AUTH_INFO);
    transcript.extend_from_slice(client_ephemeral);
    transcript.extend_from_slice(client_nonce);
    transcript.extend_from_slice(server_ephemeral);
    transcript.extend_from_slice(server_nonce);
    transcript
}

pub(crate) fn encrypt_record(
    key: &[u8; 32],
    mac_key: &[u8; 32],
    sequence: u64,
    plaintext: &[u8],
) -> Result<Vec<u8>, Error> {
    let mut rng = OsRng::default();
    let mut iv = [0u8; AES_BLOCK];
    rng.fill_bytes(&mut iv);
    let ciphertext = encrypt_aes256_cbc(key, &iv, plaintext);
    let mut header = Vec::with_capacity(4 + 8);
    header.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());
    header.extend_from_slice(&sequence.to_be_bytes());
    let mut mac_input = header.clone();
    mac_input.extend_from_slice(&iv);
    mac_input.extend_from_slice(&ciphertext);
    let mac = hmac_sha256(mac_key, &mac_input);
    let mut out = header;
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&mac);
    Ok(out)
}

pub(crate) fn decrypt_record(
    key: &[u8; 32],
    mac_key: &[u8; 32],
    expected_sequence: u64,
    frame: &[u8],
) -> Result<Vec<u8>, Error> {
    if frame.len() < 4 + 8 + AES_BLOCK + MAC_LEN {
        return Err(Error::InvalidHandshake("record too small"));
    }
    let length = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    let sequence = u64::from_be_bytes(frame[4..12].try_into().unwrap());
    if sequence != expected_sequence {
        return Err(Error::InvalidHandshake("sequence mismatch"));
    }
    let iv_start = 12;
    let iv_end = iv_start + AES_BLOCK;
    let mac_start = frame.len() - MAC_LEN;
    let ciphertext = &frame[iv_end..mac_start];
    let mac_input = frame[..mac_start].to_vec();
    let mac = hmac_sha256(mac_key, &mac_input);
    if &mac != &frame[mac_start..] {
        return Err(Error::InvalidHandshake("record mac mismatch"));
    }
    let mut iv = [0u8; AES_BLOCK];
    iv.copy_from_slice(&frame[iv_start..iv_end]);
    let plaintext = decrypt_aes256_cbc(key, &iv, ciphertext)
        .map_err(|err| Error::Crypto(format!("decrypt failed: {err}")))?;
    if plaintext.len() < length {
        return Err(Error::InvalidHandshake("plaintext shorter than advertised"));
    }
    Ok(plaintext[..length].to_vec())
}

impl From<Error> for io::Error {
    fn from(value: Error) -> Self {
        value.into_io()
    }
}

impl ClientHello {
    fn decode(frame: &[u8]) -> Result<Self, Error> {
        let mut cursor = 0usize;
        if frame.len() < 4 + 1 + 32 + 32 + 1 {
            return Err(Error::InvalidHandshake("handshake frame too small"));
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&frame[cursor..cursor + 4]);
        cursor += 4;
        let version = frame[cursor];
        cursor += 1;
        let mut client_ephemeral = [0u8; 32];
        client_ephemeral.copy_from_slice(&frame[cursor..cursor + 32]);
        cursor += 32;
        let mut client_nonce = [0u8; 32];
        client_nonce.copy_from_slice(&frame[cursor..cursor + 32]);
        cursor += 32;
        let flags = frame[cursor];
        cursor += 1;
        let certificate = if flags & 0x01 != 0 {
            let len = read_len(frame, &mut cursor, "client certificate length")?;
            if len > HANDSHAKE_MAX_LEN {
                return Err(Error::InvalidHandshake("client certificate length"));
            }
            Some(read_bytes(frame, &mut cursor, len, "client certificate")?)
        } else {
            None
        };
        let signature = if flags & 0x02 != 0 {
            let len = read_len(frame, &mut cursor, "client signature length")?;
            if len > ed25519::SIGNATURE_LENGTH {
                return Err(Error::InvalidHandshake("client signature length"));
            }
            Some(read_bytes(frame, &mut cursor, len, "client signature")?)
        } else {
            None
        };
        if cursor != frame.len() {
            return Err(Error::InvalidHandshake(
                "unexpected trailing handshake bytes",
            ));
        }
        Ok(ClientHello {
            magic,
            version,
            client_ephemeral,
            client_nonce,
            certificate,
            signature,
        })
    }

    #[allow(dead_code)]
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 1 + 32 + 32 + 1);
        out.extend_from_slice(&self.magic);
        out.push(self.version);
        out.extend_from_slice(&self.client_ephemeral);
        out.extend_from_slice(&self.client_nonce);
        let mut flags = 0u8;
        if self.certificate.is_some() {
            flags |= 0x01;
        }
        if self.signature.is_some() {
            flags |= 0x02;
        }
        out.push(flags);
        if let Some(cert) = &self.certificate {
            out.extend_from_slice(&(cert.len() as u32).to_be_bytes());
            out.extend_from_slice(cert);
        }
        if let Some(sig) = &self.signature {
            out.extend_from_slice(&(sig.len() as u32).to_be_bytes());
            out.extend_from_slice(sig);
        }
        out
    }
}

impl ServerHello {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            4 + 1 + 32 + 32 + 4 + self.certificate.len() + 4 + self.signature.len() + 1,
        );
        out.extend_from_slice(&self.magic);
        out.push(self.version);
        out.extend_from_slice(&self.server_ephemeral);
        out.extend_from_slice(&self.server_nonce);
        out.extend_from_slice(&(self.certificate.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.certificate);
        out.extend_from_slice(&(self.signature.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.signature);
        out.push(if self.client_auth_required { 1 } else { 0 });
        out
    }

    #[allow(dead_code)]
    fn decode(frame: &[u8]) -> Result<Self, Error> {
        let mut cursor = 0usize;
        if frame.len() < 4 + 1 + 32 + 32 + 4 + 4 + 1 {
            return Err(Error::InvalidHandshake("server handshake frame too small"));
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&frame[cursor..cursor + 4]);
        cursor += 4;
        let version = frame[cursor];
        cursor += 1;
        let mut server_ephemeral = [0u8; 32];
        server_ephemeral.copy_from_slice(&frame[cursor..cursor + 32]);
        cursor += 32;
        let mut server_nonce = [0u8; 32];
        server_nonce.copy_from_slice(&frame[cursor..cursor + 32]);
        cursor += 32;
        let cert_len = read_len(frame, &mut cursor, "server certificate length")?;
        let certificate = read_bytes(frame, &mut cursor, cert_len, "server certificate")?;
        let sig_len = read_len(frame, &mut cursor, "server signature length")?;
        let signature = read_bytes(frame, &mut cursor, sig_len, "server signature")?;
        if cursor >= frame.len() {
            return Err(Error::InvalidHandshake("missing server auth flag"));
        }
        let auth_flag = frame[cursor];
        cursor += 1;
        if cursor != frame.len() {
            return Err(Error::InvalidHandshake(
                "unexpected trailing server handshake bytes",
            ));
        }
        Ok(ServerHello {
            magic,
            version,
            server_ephemeral,
            server_nonce,
            certificate,
            signature,
            client_auth_required: auth_flag != 0,
        })
    }
}

fn read_len(frame: &[u8], cursor: &mut usize, label: &'static str) -> Result<usize, Error> {
    if frame.len() < *cursor + 4 {
        return Err(Error::InvalidHandshake(label));
    }
    let len = u32::from_be_bytes(frame[*cursor..*cursor + 4].try_into().unwrap()) as usize;
    *cursor += 4;
    Ok(len)
}

fn read_bytes(
    frame: &[u8],
    cursor: &mut usize,
    len: usize,
    label: &'static str,
) -> Result<Vec<u8>, Error> {
    if frame.len() < *cursor + len {
        return Err(Error::InvalidHandshake(label));
    }
    let bytes = frame[*cursor..*cursor + len].to_vec();
    *cursor += len;
    Ok(bytes)
}

pub(crate) fn parse_certificate(bytes: &[u8]) -> Result<VerifyingKey, Error> {
    let value: Value = json::from_slice(bytes).map_err(|err| Error::Encoding(err.to_string()))?;
    let map = match &value {
        Value::Object(map) => map,
        _ => return Err(Error::InvalidHandshake("certificate must be an object")),
    };
    let version = expect_u64(map, "version")?;
    if version != 1 {
        return Err(Error::InvalidHandshake("certificate version"));
    }
    let algorithm = expect_string(map, "algorithm")?;
    if !algorithm.eq_ignore_ascii_case(ed25519::ALGORITHM) {
        return Err(Error::InvalidHandshake(
            "certificate algorithm must be ed25519",
        ));
    }
    let public_key = expect_string(map, "public_key")?;
    decode_public_key(&public_key)
}

pub(crate) fn parse_signing_key(bytes: &[u8]) -> Result<SigningKey, Error> {
    let value: Value = json::from_slice(bytes).map_err(|err| Error::Encoding(err.to_string()))?;
    let map = match &value {
        Value::Object(map) => map,
        _ => return Err(Error::InvalidHandshake("private key must be an object")),
    };
    let version = expect_u64(map, "version")?;
    if version != 1 {
        return Err(Error::InvalidHandshake("private key version"));
    }
    let algorithm = expect_string(map, "algorithm")?;
    if !algorithm.eq_ignore_ascii_case(ed25519::ALGORITHM) {
        return Err(Error::InvalidHandshake(
            "private key algorithm must be ed25519",
        ));
    }
    let private_key = expect_string(map, "private_key")?;
    decode_secret_key(&private_key)
}

fn decode_public_key(encoded: &str) -> Result<VerifyingKey, Error> {
    let bytes = base64::decode_standard(encoded)
        .map_err(|err| Error::Encoding(format!("invalid certificate base64: {err}")))?;
    if bytes.len() != ed25519::PUBLIC_KEY_LENGTH {
        return Err(Error::InvalidHandshake("certificate public key length"));
    }
    let mut key = [0u8; ed25519::PUBLIC_KEY_LENGTH];
    key.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&key)
        .map_err(|err| Error::Encoding(format!("invalid certificate public key: {err}")))
}

fn decode_secret_key(encoded: &str) -> Result<SigningKey, Error> {
    let bytes = base64::decode_standard(encoded)
        .map_err(|err| Error::Encoding(format!("invalid key base64: {err}")))?;
    if bytes.len() != ed25519::SECRET_KEY_LENGTH {
        return Err(Error::InvalidHandshake("private key length"));
    }
    let mut buf = [0u8; ed25519::SECRET_KEY_LENGTH];
    buf.copy_from_slice(&bytes);
    Ok(SigningKey::from_bytes(&buf))
}

fn expect_u64(map: &Map, key: &str) -> Result<u64, Error> {
    let value = map
        .get(key)
        .ok_or_else(|| Error::InvalidHandshake("missing numeric field"))?;
    json::from_value::<u64>(value.clone()).map_err(|err| Error::Encoding(err.to_string()))
}

fn expect_string(map: &Map, key: &str) -> Result<String, Error> {
    let value = map
        .get(key)
        .ok_or_else(|| Error::InvalidHandshake("missing string field"))?;
    json::from_value::<String>(value.clone()).map_err(|err| Error::Encoding(err.to_string()))
}
