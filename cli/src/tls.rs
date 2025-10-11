use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{self, Value};
use foundation_serialization::Serialize;
use foundation_tls::ed25519_public_key_from_der;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub enum TlsCmd {
    Convert {
        cert: PathBuf,
        key: PathBuf,
        anchor: Option<PathBuf>,
        out_dir: PathBuf,
        name: String,
        force: bool,
    },
}

impl TlsCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("tls"), "tls", "TLS identity tooling")
            .subcommand(
                CommandBuilder::new(
                    CommandId("tls.convert"),
                    "convert",
                    "Convert TLS materials to JSON",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new("cert", "cert", "Path to the server certificate")
                        .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("key", "key", "Path to the private key").required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "anchor",
                    "anchor",
                    "Optional trust anchor to convert",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("out-dir", "out-dir", "Directory for JSON outputs")
                        .default("."),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("name", "name", "Base name for output files")
                        .default("identity"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "force",
                    "force",
                    "Overwrite existing files",
                )))
                .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'tls'".to_string())?;

        match name {
            "convert" => {
                let cert = required_option(sub_matches, "cert")?;
                let key = required_option(sub_matches, "key")?;
                let anchor = sub_matches.get_string("anchor");
                let out_dir = sub_matches
                    .get_string("out-dir")
                    .unwrap_or_else(|| ".".to_string());
                let name = sub_matches
                    .get_string("name")
                    .unwrap_or_else(|| "identity".to_string());
                let force = sub_matches.get_flag("force");
                Ok(TlsCmd::Convert {
                    cert: cert.into(),
                    key: key.into(),
                    anchor: anchor.map(PathBuf::from),
                    out_dir: out_dir.into(),
                    name,
                    force,
                })
            }
            other => Err(format!("unknown tls command '{other}'")),
        }
    }
}

pub fn handle(cmd: TlsCmd) -> Result<(), String> {
    match cmd {
        TlsCmd::Convert {
            cert,
            key,
            anchor,
            out_dir,
            name,
            force,
        } => convert_tls(cert, key, anchor, out_dir, name, force).map(|outputs| {
            for path in outputs {
                println!("wrote {}", path.display());
            }
        }),
    }
}

fn convert_tls(
    cert_path: PathBuf,
    key_path: PathBuf,
    anchor_path: Option<PathBuf>,
    out_dir: PathBuf,
    name: String,
    force: bool,
) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "failed to create output directory '{}': {err}",
            out_dir.display()
        )
    })?;

    let cert_bytes = fs::read(&cert_path).map_err(|err| {
        format!(
            "failed to read certificate '{}': {err}",
            cert_path.display()
        )
    })?;
    let verifying = parse_certificate(&cert_bytes).map_err(|err| {
        format!(
            "failed to parse certificate '{}': {err}",
            cert_path.display()
        )
    })?;
    let key_bytes = fs::read(&key_path)
        .map_err(|err| format!("failed to read private key '{}': {err}", key_path.display()))?;
    let signing = parse_private_key(&key_bytes).map_err(|err| {
        format!(
            "failed to parse private key '{}': {err}",
            key_path.display()
        )
    })?;

    let cert_json = render_certificate_json(&verifying);
    let key_json = render_key_json(&signing);

    let mut outputs = Vec::new();
    let cert_out = out_dir.join(format!("{}-cert.json", name));
    let key_out = out_dir.join(format!("{}-key.json", name));
    maybe_write(&cert_out, &cert_json, force)?;
    maybe_write(&key_out, &key_json, force)?;
    outputs.push(cert_out);
    outputs.push(key_out);

    if let Some(anchor_path) = anchor_path {
        let anchor_bytes = fs::read(&anchor_path).map_err(|err| {
            format!(
                "failed to read trust anchor '{}': {err}",
                anchor_path.display()
            )
        })?;
        let anchor_key = parse_certificate(&anchor_bytes).map_err(|err| {
            format!(
                "failed to parse trust anchor '{}': {err}",
                anchor_path.display()
            )
        })?;
        let anchor_json = render_anchor_json(&anchor_key);
        let anchor_out = out_dir.join(format!("{}-anchor.json", name));
        maybe_write(&anchor_out, &anchor_json, force)?;
        outputs.push(anchor_out);
    }

    Ok(outputs)
}

fn maybe_write(path: &Path, contents: &[u8], force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "refusing to overwrite existing file '{}'; pass --force to override",
            path.display()
        ));
    }
    fs::write(path, contents).map_err(|err| format!("failed to write '{}': {err}", path.display()))
}

fn parse_certificate(bytes: &[u8]) -> Result<[u8; 32], ConvertError> {
    if looks_like_json(bytes) {
        let value: Value =
            json::from_slice(bytes).map_err(|err| ConvertError::Encoding(err.to_string()))?;
        let map = value
            .as_object()
            .ok_or(ConvertError::Invalid("certificate must be an object"))?;
        let algorithm = map
            .get("algorithm")
            .and_then(Value::as_str)
            .ok_or(ConvertError::Invalid("certificate missing algorithm"))?;
        if !algorithm.eq_ignore_ascii_case("ed25519") {
            return Err(ConvertError::Invalid(
                "certificate algorithm must be ed25519",
            ));
        }
        let public_key = map
            .get("public_key")
            .and_then(Value::as_str)
            .ok_or(ConvertError::Invalid("certificate missing public_key"))?;
        let bytes = base64_fp::decode_standard(public_key)
            .map_err(|err| ConvertError::Encoding(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(ConvertError::Invalid("certificate public key length"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }

    let ders = decode_der_blobs(bytes)?;
    for der in ders {
        if let Ok(key) = ed25519_public_key_from_der(&der) {
            return Ok(key);
        }
    }
    Err(ConvertError::Invalid(
        "certificate did not contain an ed25519 key",
    ))
}

fn parse_private_key(bytes: &[u8]) -> Result<[u8; 32], ConvertError> {
    if looks_like_json(bytes) {
        let value: Value =
            json::from_slice(bytes).map_err(|err| ConvertError::Encoding(err.to_string()))?;
        let map = value
            .as_object()
            .ok_or(ConvertError::Invalid("private key must be an object"))?;
        let algorithm = map
            .get("algorithm")
            .and_then(Value::as_str)
            .ok_or(ConvertError::Invalid("private key missing algorithm"))?;
        if !algorithm.eq_ignore_ascii_case("ed25519") {
            return Err(ConvertError::Invalid(
                "private key algorithm must be ed25519",
            ));
        }
        let private_key =
            map.get("private_key")
                .and_then(Value::as_str)
                .ok_or(ConvertError::Invalid(
                    "private key missing private_key field",
                ))?;
        let bytes = base64_fp::decode_standard(private_key)
            .map_err(|err| ConvertError::Encoding(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(ConvertError::Invalid("private key length"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }

    let ders = decode_der_blobs(bytes)?;
    for der in ders {
        if let Ok(signing) = SigningKey::from_pkcs8_der(&der) {
            return Ok(signing.to_bytes());
        }
    }
    Err(ConvertError::Invalid(
        "private key did not contain an ed25519 key",
    ))
}

fn render_certificate_json(verifying: &[u8; 32]) -> Vec<u8> {
    let encoded = base64_fp::encode_standard(verifying);
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"public_key\":\"{}\"}}",
        encoded
    )
    .into_bytes()
}

fn render_anchor_json(verifying: &[u8; 32]) -> Vec<u8> {
    let entry = AnchorEntry {
        version: 1,
        allowed: vec![AnchorIdentity {
            algorithm: "ed25519",
            public_key: base64_fp::encode_standard(verifying),
        }],
    };
    json::to_vec(&entry).expect("anchor json")
}

fn render_key_json(signing: &[u8; 32]) -> Vec<u8> {
    let encoded = base64_fp::encode_standard(signing);
    format!(
        "{{\"version\":1,\"algorithm\":\"ed25519\",\"private_key\":\"{}\"}}",
        encoded
    )
    .into_bytes()
}

fn decode_der_blobs(bytes: &[u8]) -> Result<Vec<Vec<u8>>, ConvertError> {
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

fn parse_pem_blocks(input: &str) -> Result<Vec<(String, Vec<u8>)>, ConvertError> {
    let mut blocks = Vec::new();
    let mut current_label: Option<String> = None;
    let mut buffer = String::new();
    for line in input.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("-----BEGIN ") {
            if current_label.is_some() {
                return Err(ConvertError::Invalid("nested pem begin"));
            }
            if let Some(end) = rest.strip_suffix("-----") {
                current_label = Some(end.trim().to_string());
                buffer.clear();
                continue;
            }
            return Err(ConvertError::Invalid("invalid pem begin"));
        }
        if let Some(rest) = line.strip_prefix("-----END ") {
            let label = current_label
                .take()
                .ok_or(ConvertError::Invalid("pem end without begin"))?;
            if !rest.starts_with(&label) {
                return Err(ConvertError::Invalid("pem end label mismatch"));
            }
            let decoded = base64_fp::decode_standard(&buffer)
                .map_err(|err| ConvertError::Encoding(err.to_string()))?;
            blocks.push((label, decoded));
            buffer.clear();
            continue;
        }
        if current_label.is_some() {
            buffer.push_str(line);
        }
    }
    if current_label.is_some() {
        return Err(ConvertError::Invalid("unterminated pem block"));
    }
    Ok(blocks)
}

fn looks_like_json(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .skip_while(|b| b"\n\r\t ".contains(b))
        .copied()
        .map(|b| b == b'{' || b == b'[')
        .next()
        .unwrap_or(false)
}

fn required_option(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_string(name)
        .ok_or_else(|| format!("missing required option '--{name}'"))
}

#[derive(Serialize)]
struct AnchorEntry<'a> {
    version: u8,
    allowed: Vec<AnchorIdentity<'a>>,
}

#[derive(Serialize)]
struct AnchorIdentity<'a> {
    algorithm: &'a str,
    public_key: String,
}

#[derive(Debug)]
enum ConvertError {
    Io(io::Error),
    Invalid(&'static str),
    Encoding(String),
}

impl From<io::Error> for ConvertError {
    fn from(value: io::Error) -> Self {
        ConvertError::Io(value)
    }
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Io(err) => write!(f, "io error: {err}"),
            ConvertError::Invalid(msg) => write!(f, "{msg}"),
            ConvertError::Encoding(msg) => write!(f, "encoding error: {msg}"),
        }
    }
}

impl std::error::Error for ConvertError {}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn parse_certificate_json_round_trips() {
        let signing = SigningKey::generate(&mut OsRng::default());
        let verifying = signing.verifying_key();
        let json = render_certificate_json(&verifying.to_bytes());
        let parsed = parse_certificate(&json).expect("parse certificate");
        assert_eq!(parsed, verifying.to_bytes());
    }

    #[test]
    fn parse_private_key_json_round_trips() {
        let signing = SigningKey::generate(&mut OsRng::default());
        let json = render_key_json(&signing.to_bytes());
        let parsed = parse_private_key(&json).expect("parse private key");
        assert_eq!(parsed, signing.to_bytes());
    }

    #[test]
    fn looks_like_json_detects_objects() {
        assert!(looks_like_json(br#" { "k": 1 } "#));
        assert!(!looks_like_json(b"-----BEGIN CERT-----"));
    }
}
