use crate::tls::{
    self, HANDSHAKE_MAGIC, HANDSHAKE_MAX_LEN, HANDSHAKE_VERSION, build_server_transcript,
    decrypt_record, encrypt_record, parse_certificate, parse_signing_key,
};
use base64_fp::{decode_standard, encode_standard};
use crypto_suite::encryption::x25519::{PublicKey as X25519Public, SecretKey as X25519Secret};
use crypto_suite::signatures::ed25519::{
    SIGNATURE_LENGTH, Signature as EdSignature, SigningKey, VerifyingKey,
};
use foundation_tls::ed25519_public_key_from_der;
use rand::RngCore;
use rand::rngs::OsRng;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::thread;
use std::time::Duration;

const MAX_RECORD_CHUNK: usize = 16 * 1024;

#[derive(Debug)]
pub enum TlsConnectorError {
    Io(io::Error),
    InvalidIdentity(&'static str),
    InvalidCertificate(&'static str),
    VerificationFailed(&'static str),
    Encoding(String),
    Crypto(String),
}

impl fmt::Display for TlsConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TlsConnectorError::Io(err) => write!(f, "io error: {err}"),
            TlsConnectorError::InvalidIdentity(msg) => write!(f, "invalid identity: {msg}"),
            TlsConnectorError::InvalidCertificate(msg) => {
                write!(f, "invalid certificate: {msg}")
            }
            TlsConnectorError::VerificationFailed(msg) => {
                write!(f, "certificate verification failed: {msg}")
            }
            TlsConnectorError::Encoding(msg) => write!(f, "encoding error: {msg}"),
            TlsConnectorError::Crypto(msg) => write!(f, "crypto error: {msg}"),
        }
    }
}

impl std::error::Error for TlsConnectorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TlsConnectorError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for TlsConnectorError {
    fn from(value: io::Error) -> Self {
        TlsConnectorError::Io(value)
    }
}

impl From<tls::Error> for TlsConnectorError {
    fn from(value: tls::Error) -> Self {
        match value {
            tls::Error::Io(err) => TlsConnectorError::Io(err),
            tls::Error::Encoding(msg) => TlsConnectorError::Encoding(msg),
            tls::Error::InvalidHandshake(msg) => TlsConnectorError::InvalidCertificate(msg),
            tls::Error::UnknownClient => {
                TlsConnectorError::VerificationFailed("client certificate rejected")
            }
            tls::Error::SignatureFailed => {
                TlsConnectorError::VerificationFailed("signature validation failed")
            }
            tls::Error::Crypto(msg) => TlsConnectorError::Crypto(msg),
        }
    }
}

#[derive(Clone)]
struct ClientIdentity {
    certificate: Vec<u8>,
    signing: SigningKey,
}

#[derive(Clone)]
pub struct TlsConnector {
    identity: Option<ClientIdentity>,
    anchors: Vec<VerifyingKey>,
    allow_invalid: bool,
}

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector")
            .field("identity", &self.identity.is_some())
            .field("anchors", &self.anchors.len())
            .field("allow_invalid", &self.allow_invalid)
            .finish()
    }
}

impl TlsConnector {
    pub fn builder() -> TlsConnectorBuilder {
        TlsConnectorBuilder::default()
    }

    pub fn connect(
        &self,
        _host: &str,
        mut stream: TcpStream,
    ) -> Result<ClientTlsStream, TlsConnectorError> {
        stream.set_nodelay(true).ok();
        let mut rng = OsRng::default();
        let secret = X25519Secret::generate(&mut rng);
        let client_ephemeral = secret.public_key().to_bytes();
        let mut client_nonce = [0u8; 32];
        rng.fill_bytes(&mut client_nonce);

        let (certificate, signature) = if let Some(identity) = &self.identity {
            let mut message = Vec::with_capacity(client_ephemeral.len() + client_nonce.len());
            message.extend_from_slice(&client_ephemeral);
            message.extend_from_slice(&client_nonce);
            let sig = identity.signing.sign(&message);
            (
                Some(identity.certificate.clone()),
                Some(sig.to_bytes().to_vec()),
            )
        } else {
            (None, None)
        };

        let hello = encode_client_hello(
            &client_ephemeral,
            &client_nonce,
            certificate.as_deref(),
            signature.as_deref(),
        );
        let len = (hello.len() as u32).to_be_bytes();
        blocking_write_all(&mut stream, &len)?;
        blocking_write_all(&mut stream, &hello)?;
        stream.flush()?;

        let mut len_buf = [0u8; 4];
        blocking_read_exact(&mut stream, &mut len_buf)?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len > HANDSHAKE_MAX_LEN {
            return Err(TlsConnectorError::InvalidCertificate(
                "server handshake too large",
            ));
        }
        let mut frame = vec![0u8; frame_len];
        blocking_read_exact(&mut stream, &mut frame)?;
        let server = ServerHelloFrame::decode(&frame)?;
        if server.magic != *HANDSHAKE_MAGIC {
            return Err(TlsConnectorError::InvalidCertificate(
                "invalid handshake magic",
            ));
        }
        if server.version != HANDSHAKE_VERSION {
            return Err(TlsConnectorError::InvalidCertificate(
                "unsupported handshake version",
            ));
        }
        if server.client_auth_required && self.identity.is_none() {
            return Err(TlsConnectorError::VerificationFailed(
                "server requires client authentication",
            ));
        }

        let server_cert = parse_server_certificate(&server.certificate)?;
        validate_server_certificate(&server_cert, &self.anchors, self.allow_invalid)?;

        let server_public = X25519Public::from_bytes(&server.server_ephemeral)
            .map_err(|_| TlsConnectorError::InvalidCertificate("invalid server ephemeral key"))?;
        let shared = secret.diffie_hellman(&server_public).to_bytes();
        let session = tls::SessionKeys::derive(&shared, &client_nonce, &server.server_nonce)?;

        let mut sig_bytes = [0u8; SIGNATURE_LENGTH];
        if server.signature.len() != sig_bytes.len() {
            return Err(TlsConnectorError::InvalidCertificate(
                "server signature length",
            ));
        }
        sig_bytes.copy_from_slice(&server.signature);
        let signature = EdSignature::from_bytes(&sig_bytes);
        let transcript = build_server_transcript(
            &client_ephemeral,
            &client_nonce,
            &server.server_ephemeral,
            &server.server_nonce,
        );
        server_cert
            .verifying
            .verify(&transcript, &signature)
            .map_err(|_| TlsConnectorError::VerificationFailed("server handshake signature"))?;

        Ok(ClientTlsStream::new(stream, session))
    }
}

#[derive(Default)]
pub struct TlsConnectorBuilder {
    identity: Option<ClientIdentity>,
    anchors: Vec<VerifyingKey>,
    allow_invalid: bool,
}

impl TlsConnectorBuilder {
    pub fn identity_from_files(
        &mut self,
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<&mut Self, TlsConnectorError> {
        let cert_bytes = fs::read(cert_path)?;
        let key_bytes = fs::read(key_path)?;
        let identity = parse_identity(&cert_bytes, &key_bytes)?;
        self.identity = Some(identity);
        Ok(self)
    }

    pub fn add_trust_anchor_from_file(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<&mut Self, TlsConnectorError> {
        let bytes = fs::read(path)?;
        let anchors = parse_trust_anchors(&bytes)?;
        self.anchors.extend(anchors);
        Ok(self)
    }

    pub fn danger_accept_invalid_certs(&mut self, allow: bool) -> &mut Self {
        self.allow_invalid = allow;
        self
    }

    pub fn build(self) -> Result<TlsConnector, TlsConnectorError> {
        Ok(TlsConnector {
            identity: self.identity,
            anchors: dedupe_verifying_keys(self.anchors),
            allow_invalid: self.allow_invalid,
        })
    }
}

fn dedupe_verifying_keys(keys: Vec<VerifyingKey>) -> Vec<VerifyingKey> {
    let mut out = Vec::new();
    for key in keys.into_iter() {
        let candidate = key.to_bytes();
        if out
            .iter()
            .any(|existing: &VerifyingKey| existing.to_bytes() == candidate)
        {
            continue;
        }
        out.push(key);
    }
    out
}

struct ParsedServerCertificate {
    verifying: VerifyingKey,
    kind: CertificateKind,
}

enum CertificateKind {
    Json,
    Der(Vec<u8>),
}

fn parse_server_certificate(bytes: &[u8]) -> Result<ParsedServerCertificate, TlsConnectorError> {
    if let Ok(verifying) = parse_certificate(bytes) {
        return Ok(ParsedServerCertificate {
            verifying,
            kind: CertificateKind::Json,
        });
    }
    let der = decode_der_blob(bytes)?;
    let key_bytes = ed25519_public_key_from_der(&der).map_err(|err| {
        TlsConnectorError::InvalidCertificate(match err {
            foundation_tls::CertificateError::InvalidPublicKey => "invalid ed25519 public key",
            foundation_tls::CertificateError::MissingPublicKey => "missing ed25519 public key",
            _ => "malformed certificate",
        })
    })?;
    let verifying = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|err| TlsConnectorError::Encoding(err.to_string()))?;
    Ok(ParsedServerCertificate {
        verifying,
        kind: CertificateKind::Der(der),
    })
}

fn validate_server_certificate(
    cert: &ParsedServerCertificate,
    anchors: &[VerifyingKey],
    allow_invalid: bool,
) -> Result<(), TlsConnectorError> {
    if anchors.is_empty() {
        if allow_invalid {
            return Ok(());
        }
        return Err(TlsConnectorError::VerificationFailed(
            "no trust anchors configured",
        ));
    }
    match &cert.kind {
        CertificateKind::Json => {
            let server_bytes = cert.verifying.to_bytes();
            if anchors
                .iter()
                .any(|anchor| anchor.to_bytes() == server_bytes)
            {
                Ok(())
            } else {
                Err(TlsConnectorError::VerificationFailed(
                    "server certificate not in trust anchors",
                ))
            }
        }
        CertificateKind::Der(der) => {
            let (tbs, signature) = extract_tbs_and_signature(der)?;
            for anchor in anchors {
                if anchor
                    .verify(tbs, &EdSignature::from_bytes(&signature))
                    .is_ok()
                {
                    return Ok(());
                }
            }
            Err(TlsConnectorError::VerificationFailed(
                "server certificate signature rejected",
            ))
        }
    }
}

fn parse_identity(
    cert_bytes: &[u8],
    key_bytes: &[u8],
) -> Result<ClientIdentity, TlsConnectorError> {
    let (certificate, verifying) = if let Ok(verifying) = parse_certificate(cert_bytes) {
        (cert_bytes.to_vec(), verifying)
    } else {
        let der = decode_der_blob(cert_bytes)?;
        let key_bytes = ed25519_public_key_from_der(&der)
            .map_err(|_| TlsConnectorError::InvalidCertificate("invalid certificate public key"))?;
        let verifying = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|err| TlsConnectorError::Encoding(err.to_string()))?;
        (render_certificate_json(&verifying), verifying)
    };

    let signing = if looks_like_json(key_bytes) {
        parse_signing_key(key_bytes)
            .map_err(|_| TlsConnectorError::InvalidIdentity("invalid signing key json"))?
    } else {
        let der = decode_der_blob(key_bytes)?;
        SigningKey::from_pkcs8_der(&der)
            .map_err(|_| TlsConnectorError::InvalidIdentity("invalid pkcs8 private key"))?
    };
    if signing.verifying_key().to_bytes() != verifying.to_bytes() {
        return Err(TlsConnectorError::InvalidIdentity(
            "certificate public key does not match private key",
        ));
    }
    Ok(ClientIdentity {
        certificate,
        signing,
    })
}

fn parse_trust_anchors(bytes: &[u8]) -> Result<Vec<VerifyingKey>, TlsConnectorError> {
    if let Ok(verifying) = parse_certificate(bytes) {
        return Ok(vec![verifying]);
    }
    let ders = decode_all_der_blobs(bytes)?;
    let mut out = Vec::new();
    for der in ders {
        let key_bytes = ed25519_public_key_from_der(&der)
            .map_err(|_| TlsConnectorError::InvalidCertificate("invalid trust anchor"))?;
        let verifying = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|err| TlsConnectorError::Encoding(err.to_string()))?;
        out.push(verifying);
    }
    if out.is_empty() {
        Err(TlsConnectorError::InvalidCertificate(
            "trust anchor file did not contain certificates",
        ))
    } else {
        Ok(out)
    }
}

fn looks_like_json(bytes: &[u8]) -> bool {
    let trimmed = bytes
        .iter()
        .skip_while(|b| b"\n\r\t ".contains(b))
        .copied()
        .next();
    matches!(trimmed, Some(b'{') | Some(b'['))
}

fn render_certificate_json(verifying: &VerifyingKey) -> Vec<u8> {
    let encoded = encode_standard(&verifying.to_bytes());
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
        encoded
    )
    .into_bytes()
}

fn decode_der_blob(bytes: &[u8]) -> Result<Vec<u8>, TlsConnectorError> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        let mut blobs = Vec::new();
        for (label, blob) in parse_pem_blocks(text)? {
            if label.eq_ignore_ascii_case("certificate")
                || label.eq_ignore_ascii_case("private key")
            {
                blobs.push(blob);
            }
        }
        if !blobs.is_empty() {
            return Ok(blobs.into_iter().next().unwrap());
        }
    }
    Ok(bytes.to_vec())
}

fn decode_all_der_blobs(bytes: &[u8]) -> Result<Vec<Vec<u8>>, TlsConnectorError> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        let mut blobs = Vec::new();
        for (_, blob) in parse_pem_blocks(text)? {
            blobs.push(blob);
        }
        if !blobs.is_empty() {
            return Ok(blobs);
        }
    }
    Ok(vec![bytes.to_vec()])
}

fn parse_pem_blocks(input: &str) -> Result<Vec<(String, Vec<u8>)>, TlsConnectorError> {
    let mut blocks = Vec::new();
    let mut current_label = None;
    let mut buffer = String::new();
    for line in input.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("-----BEGIN ") {
            if current_label.is_some() {
                return Err(TlsConnectorError::InvalidCertificate("nested pem begin"));
            }
            if let Some(end) = rest.strip_suffix("-----") {
                current_label = Some(end.trim().to_string());
                buffer.clear();
                continue;
            }
            return Err(TlsConnectorError::InvalidCertificate("invalid pem begin"));
        }
        if let Some(rest) = line.strip_prefix("-----END ") {
            let label = current_label
                .take()
                .ok_or(TlsConnectorError::InvalidCertificate(
                    "pem end without begin",
                ))?;
            if !rest.starts_with(&label) {
                return Err(TlsConnectorError::InvalidCertificate(
                    "pem end label mismatch",
                ));
            }
            let decoded = decode_standard(&buffer)
                .map_err(|err| TlsConnectorError::Encoding(err.to_string()))?;
            blocks.push((label, decoded));
            buffer.clear();
            continue;
        }
        if current_label.is_some() {
            buffer.push_str(line);
        }
    }
    if current_label.is_some() {
        return Err(TlsConnectorError::InvalidCertificate(
            "unterminated pem block",
        ));
    }
    Ok(blocks)
}

fn encode_client_hello(
    client_ephemeral: &[u8; 32],
    client_nonce: &[u8; 32],
    certificate: Option<&[u8]>,
    signature: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 32 + 32 + 1);
    out.extend_from_slice(HANDSHAKE_MAGIC);
    out.push(HANDSHAKE_VERSION);
    out.extend_from_slice(client_ephemeral);
    out.extend_from_slice(client_nonce);
    let mut flags = 0u8;
    if certificate.is_some() {
        flags |= 0x01;
    }
    if signature.is_some() {
        flags |= 0x02;
    }
    out.push(flags);
    if let Some(cert) = certificate {
        out.extend_from_slice(&(cert.len() as u32).to_be_bytes());
        out.extend_from_slice(cert);
    }
    if let Some(sig) = signature {
        out.extend_from_slice(&(sig.len() as u32).to_be_bytes());
        out.extend_from_slice(sig);
    }
    out
}

struct ServerHelloFrame {
    magic: [u8; 4],
    version: u8,
    server_ephemeral: [u8; 32],
    server_nonce: [u8; 32],
    certificate: Vec<u8>,
    signature: Vec<u8>,
    client_auth_required: bool,
}

impl ServerHelloFrame {
    fn decode(frame: &[u8]) -> Result<Self, TlsConnectorError> {
        if frame.len() < 4 + 1 + 32 + 32 + 4 + 4 + 1 {
            return Err(TlsConnectorError::InvalidCertificate(
                "handshake frame too small",
            ));
        }
        let mut cursor = 0usize;
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
        let cert_len = read_len(frame, &mut cursor, "certificate length")?;
        let certificate = read_bytes(frame, &mut cursor, cert_len, "certificate")?;
        let sig_len = read_len(frame, &mut cursor, "signature length")?;
        let signature = read_bytes(frame, &mut cursor, sig_len, "signature")?;
        if cursor >= frame.len() {
            return Err(TlsConnectorError::InvalidCertificate(
                "missing client auth flag",
            ));
        }
        let client_auth_required = frame[cursor] != 0;
        Ok(ServerHelloFrame {
            magic,
            version,
            server_ephemeral,
            server_nonce,
            certificate,
            signature,
            client_auth_required,
        })
    }
}

fn read_len(
    frame: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<usize, TlsConnectorError> {
    if *cursor + 4 > frame.len() {
        return Err(TlsConnectorError::InvalidCertificate(field));
    }
    let len = u32::from_be_bytes(frame[*cursor..*cursor + 4].try_into().unwrap()) as usize;
    *cursor += 4;
    Ok(len)
}

fn read_bytes(
    frame: &[u8],
    cursor: &mut usize,
    len: usize,
    field: &'static str,
) -> Result<Vec<u8>, TlsConnectorError> {
    if *cursor + len > frame.len() {
        return Err(TlsConnectorError::InvalidCertificate(field));
    }
    let out = frame[*cursor..*cursor + len].to_vec();
    *cursor += len;
    Ok(out)
}

fn extract_tbs_and_signature(der: &[u8]) -> Result<(&[u8], [u8; 64]), TlsConnectorError> {
    if der.is_empty() || der[0] != 0x30 {
        return Err(TlsConnectorError::InvalidCertificate(
            "certificate root must be sequence",
        ));
    }
    let mut cursor = 1usize;
    let (cert_len, consumed) = decode_der_length(&der[cursor..])?;
    cursor += consumed;
    let cert_end = cursor
        .checked_add(cert_len)
        .ok_or_else(|| TlsConnectorError::InvalidCertificate("certificate length overflow"))?;
    if cert_end > der.len() {
        return Err(TlsConnectorError::InvalidCertificate(
            "certificate truncated",
        ));
    }
    let tbs_start = cursor;
    if der.get(cursor) != Some(&0x30) {
        return Err(TlsConnectorError::InvalidCertificate(
            "tbs must be sequence",
        ));
    }
    cursor += 1;
    let (tbs_len, len_bytes) = decode_der_length(&der[cursor..])?;
    cursor += len_bytes;
    let tbs_end = cursor
        .checked_add(tbs_len)
        .ok_or_else(|| TlsConnectorError::InvalidCertificate("tbs length overflow"))?;
    if tbs_end > der.len() {
        return Err(TlsConnectorError::InvalidCertificate("tbs truncated"));
    }
    cursor = tbs_end;
    if der.get(cursor) != Some(&0x30) {
        return Err(TlsConnectorError::InvalidCertificate(
            "algorithm must be sequence",
        ));
    }
    cursor += 1;
    let (alg_len, alg_bytes) = decode_der_length(&der[cursor..])?;
    cursor += alg_bytes;
    cursor = cursor
        .checked_add(alg_len)
        .ok_or_else(|| TlsConnectorError::InvalidCertificate("algorithm length overflow"))?;
    if cursor > der.len() {
        return Err(TlsConnectorError::InvalidCertificate("algorithm truncated"));
    }
    if der.get(cursor) != Some(&0x03) {
        return Err(TlsConnectorError::InvalidCertificate(
            "signature must be bit string",
        ));
    }
    cursor += 1;
    let (sig_len, sig_bytes) = decode_der_length(&der[cursor..])?;
    cursor += sig_bytes;
    if cursor + sig_len > der.len() {
        return Err(TlsConnectorError::InvalidCertificate("signature truncated"));
    }
    if sig_len < 1 + 64 {
        return Err(TlsConnectorError::InvalidCertificate(
            "signature body too small",
        ));
    }
    if der[cursor] != 0 {
        return Err(TlsConnectorError::InvalidCertificate("signature padding"));
    }
    cursor += 1;
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&der[cursor..cursor + 64]);
    let tbs_slice = &der[tbs_start..tbs_end];
    Ok((tbs_slice, signature))
}

fn decode_der_length(input: &[u8]) -> Result<(usize, usize), TlsConnectorError> {
    if input.is_empty() {
        return Err(TlsConnectorError::InvalidCertificate("missing length"));
    }
    let first = input[0];
    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }
    let count = (first & 0x7F) as usize;
    if count == 0 || count > input.len().saturating_sub(1) {
        return Err(TlsConnectorError::InvalidCertificate(
            "invalid length encoding",
        ));
    }
    let mut value = 0usize;
    for &byte in &input[1..=count] {
        value = value
            .checked_mul(256)
            .ok_or_else(|| TlsConnectorError::InvalidCertificate("length overflow"))?
            + byte as usize;
    }
    Ok((value, 1 + count))
}

#[derive(Debug)]
pub struct ClientTlsStream {
    stream: TcpStream,
    session: tls::SessionKeys,
    pending: Vec<u8>,
    read_seq: u64,
    write_seq: u64,
    eof: bool,
}

impl ClientTlsStream {
    fn new(stream: TcpStream, session: tls::SessionKeys) -> Self {
        Self {
            stream,
            session,
            pending: Vec::new(),
            read_seq: 0,
            write_seq: 0,
            eof: false,
        }
    }

    fn read_record(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0u8; 12];
        match blocking_read_exact(&mut self.stream, &mut header) {
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
            Ok(()) => {}
        }
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let padded = ((length / tls::AES_BLOCK) + 1) * tls::AES_BLOCK;
        let mut iv = [0u8; tls::AES_BLOCK];
        blocking_read_exact(&mut self.stream, &mut iv)?;
        let mut ciphertext = vec![0u8; padded];
        blocking_read_exact(&mut self.stream, &mut ciphertext)?;
        let mut mac = [0u8; tls::MAC_LEN];
        blocking_read_exact(&mut self.stream, &mut mac)?;
        let mut frame = Vec::with_capacity(12 + tls::AES_BLOCK + padded + tls::MAC_LEN);
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&iv);
        frame.extend_from_slice(&ciphertext);
        frame.extend_from_slice(&mac);
        let plain = decrypt_record(
            &self.session.server_write,
            &self.session.server_mac,
            self.read_seq,
            &frame,
        )?;
        self.read_seq = self.read_seq.wrapping_add(1);
        Ok(Some(plain))
    }

    fn write_record(&mut self, chunk: &[u8]) -> io::Result<()> {
        let frame = encrypt_record(
            &self.session.client_write,
            &self.session.client_mac,
            self.write_seq,
            chunk,
        )?;
        blocking_write_all(&mut self.stream, &frame)?;
        self.write_seq = self.write_seq.wrapping_add(1);
        Ok(())
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        let frame = encrypt_record(
            &self.session.client_write,
            &self.session.client_mac,
            self.write_seq,
            &[],
        )?;
        blocking_write_all(&mut self.stream, &frame)?;
        self.stream.flush()?;
        self.stream.shutdown(std::net::Shutdown::Both)
    }

    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

impl Read for ClientTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if !self.pending.is_empty() {
            let to_copy = buf.len().min(self.pending.len());
            buf[..to_copy].copy_from_slice(&self.pending[..to_copy]);
            if to_copy < self.pending.len() {
                self.pending.drain(..to_copy);
            } else {
                self.pending.clear();
            }
            return Ok(to_copy);
        }
        if self.eof {
            return Ok(0);
        }
        match self.read_record()? {
            Some(plain) if plain.is_empty() => {
                self.eof = true;
                Ok(0)
            }
            Some(plain) => {
                let to_copy = buf.len().min(plain.len());
                buf[..to_copy].copy_from_slice(&plain[..to_copy]);
                if to_copy < plain.len() {
                    self.pending = plain[to_copy..].to_vec();
                }
                Ok(to_copy)
            }
            None => {
                self.eof = true;
                Ok(0)
            }
        }
    }
}

impl Write for ClientTlsStream {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        let mut written = 0usize;
        while !buf.is_empty() {
            let chunk = buf.len().min(MAX_RECORD_CHUNK);
            self.write_record(&buf[..chunk])?;
            buf = &buf[chunk..];
            written += chunk;
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn parse_bool_env(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Build a TLS connector from environment variables following the provided
/// `prefix`. Callers can specify any combination of:
///
/// * `<PREFIX>_CERT` / `<PREFIX>_KEY` for a client identity pair
/// * `<PREFIX>_CA` pointing to a trust anchor bundle (PEM or JSON)
/// * `<PREFIX>_INSECURE` set to `1`, `true`, `yes`, or `on` to allow invalid
///   server certificates when no trust anchors are available.
pub fn tls_connector_from_env(prefix: &str) -> Result<Option<TlsConnector>, TlsConnectorError> {
    let cert_var = format!("{prefix}_CERT");
    let key_var = format!("{prefix}_KEY");
    let ca_var = format!("{prefix}_CA");
    let insecure_var = format!("{prefix}_INSECURE");

    let cert = env::var(&cert_var).ok();
    let key = env::var(&key_var).ok();
    let ca = env::var(&ca_var).ok();
    let insecure = env::var(&insecure_var)
        .ok()
        .map(|value| parse_bool_env(&value))
        .unwrap_or(false);

    if cert.is_none() && key.is_none() && ca.is_none() && !insecure {
        return Ok(None);
    }

    if cert.is_some() ^ key.is_some() {
        return Err(TlsConnectorError::InvalidIdentity(
            "tls identity requires both certificate and key",
        ));
    }

    let mut builder = TlsConnector::builder();
    if let (Some(cert_path), Some(key_path)) = (cert.as_deref(), key.as_deref()) {
        builder.identity_from_files(cert_path, key_path)?;
    }

    let mut has_anchor = false;
    if let Some(ca_path) = ca.as_deref() {
        builder.add_trust_anchor_from_file(ca_path)?;
        has_anchor = true;
    }

    let allow_invalid = if has_anchor { insecure } else { true };
    if !has_anchor && !insecure {
        eprintln!(
            "httpd::tls_client: allowing invalid certificates for prefix {prefix}; set {prefix}_CA or {prefix}_INSECURE=1"
        );
    }
    builder.danger_accept_invalid_certs(allow_invalid);

    builder.build().map(Some)
}

/// Attempt to construct a TLS connector from each prefix in order, returning
/// the first successful configuration.
pub fn tls_connector_from_env_any(
    prefixes: &[&str],
) -> Result<Option<TlsConnector>, TlsConnectorError> {
    for prefix in prefixes {
        if let Some(connector) = tls_connector_from_env(prefix)? {
            return Ok(Some(connector));
        }
    }
    Ok(None)
}

fn is_would_block(err: &io::Error) -> bool {
    err.kind() == ErrorKind::WouldBlock
}

fn blocking_read(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match stream.read(buf) {
            Ok(0) => return Ok(0),
            Ok(n) => return Ok(n),
            Err(err) if is_would_block(&err) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(err) => return Err(err),
        }
    }
}

fn blocking_read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        let n = blocking_read(stream, &mut buf[offset..])?;
        if n == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "tcp stream closed while reading",
            ));
        }
        offset += n;
    }
    Ok(())
}

fn blocking_write_all(stream: &mut TcpStream, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        match stream.write(buf) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "tcp stream closed while writing",
                ));
            }
            Ok(n) => buf = &buf[n..],
            Err(err) if is_would_block(&err) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}
