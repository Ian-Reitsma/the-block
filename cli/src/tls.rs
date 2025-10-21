use crate::http_client;
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::signatures::ed25519::SigningKey;
use foundation_serialization::json::{self, Map as JsonMap, Value};
use foundation_serialization::serde::de::{self, DeserializeOwned, Visitor};
use foundation_serialization::serde::ser::SerializeStruct;
use foundation_serialization::serde::{Deserializer, Serializer};
use foundation_serialization::{Deserialize, Serialize};
use foundation_time::{Duration, UtcDateTime};
use foundation_tls::ed25519_public_key_from_der;
use httpd::Method;
use std::borrow::Cow;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub enum TlsCmd {
    Convert {
        cert: PathBuf,
        key: PathBuf,
        anchor: Option<PathBuf>,
        out_dir: PathBuf,
        name: String,
        force: bool,
    },
    Stage {
        input: PathBuf,
        name: String,
        services: Vec<ServiceTarget>,
        force: bool,
        env_file: Option<PathBuf>,
    },
    Status {
        aggregator: String,
        include_latest: bool,
        output: StatusOutput,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusOutput {
    Text,
    Json,
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
            .subcommand(
                CommandBuilder::new(
                    CommandId("tls.stage"),
                    "stage",
                    "Stage converted identities for multiple services",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "input",
                        "input",
                        "Directory containing convert outputs (e.g. identity-cert.json)",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("name", "name", "Base name for converted assets")
                        .default("identity"),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "service",
                        "service",
                        "Service spec as label[:mode]=/path/to/stage (mode: none|required|optional)",
                    )
                    .multiple(true)
                    .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "env-file",
                    "env-file",
                    "Optional file to write environment exports",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "force",
                    "force",
                    "Overwrite existing staged files",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("tls.status"),
                    "status",
                    "Summarize TLS warning retention health",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "aggregator",
                        "aggregator",
                        "Metrics aggregator base URL",
                    )
                    .default("http://localhost:9000"),
                ))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "latest",
                    "latest",
                    "Include the latest TLS warning snapshots",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Render the status payload as JSON",
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
            "stage" => {
                let input = required_option(sub_matches, "input")?;
                let name = sub_matches
                    .get_string("name")
                    .unwrap_or_else(|| "identity".to_string());
                let specs = sub_matches.get_strings("service");
                if specs.is_empty() {
                    return Err("at least one --service spec is required".to_string());
                }
                let mut services = Vec::new();
                for spec in specs {
                    services.push(spec.parse()?);
                }
                let force = sub_matches.get_flag("force");
                let env_file = sub_matches.get_string("env-file");
                Ok(TlsCmd::Stage {
                    input: input.into(),
                    name,
                    services,
                    force,
                    env_file: env_file.map(PathBuf::from),
                })
            }
            "status" => {
                let aggregator = sub_matches
                    .get_string("aggregator")
                    .unwrap_or_else(|| "http://localhost:9000".to_string());
                let include_latest = sub_matches.get_flag("latest");
                let json = sub_matches.get_flag("json");
                let output = if json {
                    StatusOutput::Json
                } else {
                    StatusOutput::Text
                };
                Ok(TlsCmd::Status {
                    aggregator,
                    include_latest,
                    output,
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
        TlsCmd::Stage {
            input,
            name,
            services,
            force,
            env_file,
        } => stage_tls(input, name, services, force, env_file).map(|outputs| {
            for path in outputs {
                println!("staged {}", path.display());
            }
        }),
        TlsCmd::Status {
            aggregator,
            include_latest,
            output,
        } => status_tls(aggregator, include_latest, output),
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

    let cert_not_after = extract_certificate_not_after(&cert_bytes).map_err(|err| {
        format!(
            "failed to parse certificate metadata '{}': {err}",
            cert_path.display()
        )
    })?;
    let cert_json = render_certificate_json(&verifying, cert_not_after)
        .map_err(|err| format!("failed to encode certificate JSON: {err}"))?;
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

fn stage_tls(
    input_dir: PathBuf,
    name: String,
    services: Vec<ServiceTarget>,
    force: bool,
    env_file: Option<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    if services.is_empty() {
        return Err("at least one --service spec is required".to_string());
    }

    let cert_src = input_dir.join(format!("{}-cert.json", name));
    let key_src = input_dir.join(format!("{}-key.json", name));
    if !cert_src.exists() {
        return Err(format!(
            "missing converted certificate '{}'; run tls convert first",
            cert_src.display()
        ));
    }
    if !key_src.exists() {
        return Err(format!(
            "missing converted private key '{}'; run tls convert first",
            key_src.display()
        ));
    }
    let anchor_src = input_dir.join(format!("{}-anchor.json", name));
    let cert_bytes = fs::read(&cert_src)
        .map_err(|err| format!("failed to read '{}': {err}", cert_src.display()))?;
    let key_bytes = fs::read(&key_src)
        .map_err(|err| format!("failed to read '{}': {err}", key_src.display()))?;
    let anchor_bytes = if anchor_src.exists() {
        Some(
            fs::read(&anchor_src)
                .map_err(|err| format!("failed to read '{}': {err}", anchor_src.display()))?,
        )
    } else {
        None
    };

    let cert_not_after = extract_certificate_not_after(&cert_bytes)
        .map_err(|err| format!("failed to parse certificate metadata: {err}"))?;
    let cert_not_after_str = cert_not_after
        .map(|ts| {
            format_timestamp(ts)
                .map_err(|err| format!("failed to encode certificate expiry: {err}"))
        })
        .transpose()?;
    let renewal_window_days = cert_not_after.map(|_| 14u32);
    let renewal_reminder_str = cert_not_after
        .map(|ts| ts - Duration::days(14))
        .map(|ts| {
            format_timestamp(ts).map_err(|err| format!("failed to encode renewal reminder: {err}"))
        })
        .transpose()?;
    let staged_at = UtcDateTime::now();
    let staged_at_str = format_timestamp(staged_at)
        .map_err(|err| format!("failed to encode staging timestamp: {err}"))?;

    let mut staged_paths = Vec::new();
    let mut env_lines = env_file.as_ref().map(|_| Vec::new());
    for service in services {
        fs::create_dir_all(&service.directory).map_err(|err| {
            format!(
                "failed to create directory for service '{}': {}",
                service.label, err
            )
        })?;

        let cert_dest = service.directory.join("cert.json");
        let key_dest = service.directory.join("key.json");
        maybe_write(&cert_dest, &cert_bytes, force)?;
        maybe_write(&key_dest, &key_bytes, force)?;
        staged_paths.push(cert_dest.clone());
        staged_paths.push(key_dest.clone());

        let directory_path = canonical_path(&service.directory);
        let cert_path_str = canonical_path(&cert_dest);
        let key_path_str = canonical_path(&key_dest);
        let mut staged_files = vec![cert_path_str.clone(), key_path_str.clone()];
        let mut env_entries = Vec::new();

        if let Some(lines) = env_lines.as_mut() {
            lines.push(format!("# {} TLS assets", service.label));
        }

        let cert_var = format!("{}_CERT", service.env_prefix);
        let key_var = format!("{}_KEY", service.env_prefix);
        env_entries.push((cert_var.clone(), cert_path_str.clone()));
        env_entries.push((key_var.clone(), key_path_str.clone()));

        if let Some(lines) = env_lines.as_mut() {
            lines.push(format!("export {}={}", cert_var, cert_path_str));
            lines.push(format!("export {}={}", key_var, key_path_str));
        }

        if service.mode.requires_anchor() {
            let expected = match service.mode {
                ClientAuthMode::Required => "client_ca.json",
                ClientAuthMode::Optional => "client_ca_optional.json",
                ClientAuthMode::None => unreachable!(),
            };
            let Some(anchor_bytes) = anchor_bytes.as_ref() else {
                return Err(format!(
                    "service '{}' requires client auth assets but '{}' is missing",
                    service.label,
                    anchor_src.display()
                ));
            };
            let dest = service.directory.join(expected);
            maybe_write(&dest, anchor_bytes, force)?;
            let anchor_path = canonical_path(&dest);
            let key = match service.mode {
                ClientAuthMode::Required => format!("{}_CLIENT_CA", service.env_prefix),
                ClientAuthMode::Optional => {
                    format!("{}_CLIENT_CA_OPTIONAL", service.env_prefix)
                }
                ClientAuthMode::None => unreachable!(),
            };
            if let Some(lines) = env_lines.as_mut() {
                lines.push(format!("export {}={}", key, anchor_path.clone()));
            }
            env_entries.push((key, anchor_path.clone()));
            staged_files.push(anchor_path);
            staged_paths.push(dest);
        } else if let Some(lines) = env_lines.as_mut() {
            lines.push(format!(
                "# {} does not require client auth",
                service.env_prefix
            ));
        }

        let manifest = ServiceManifest {
            version: 1,
            generated_at: staged_at_str.clone(),
            service: service.label.clone(),
            directory: directory_path,
            env_prefix: service.env_prefix.clone(),
            client_auth: service.mode.as_str().to_string(),
            staged_files,
            env_exports: env_entries
                .iter()
                .map(|(key, value)| EnvExport {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
            renewal_timestamp: cert_not_after_str.clone(),
            renewal_reminder: renewal_reminder_str.clone(),
            renewal_window_days,
        };

        let manifest_json = json::to_vec(&manifest)
            .map_err(|err| format!("failed to encode manifest for '{}': {err}", service.label))?;
        let manifest_json_path = service.directory.join("tls-manifest.json");
        maybe_write(&manifest_json_path, &manifest_json, force)?;
        staged_paths.push(manifest_json_path.clone());

        let manifest_yaml = render_manifest_yaml(&manifest);
        let manifest_yaml_path = service.directory.join("tls-manifest.yaml");
        maybe_write(&manifest_yaml_path, manifest_yaml.as_bytes(), force)?;
        staged_paths.push(manifest_yaml_path);

        if let Some(lines) = env_lines.as_mut() {
            lines.push(String::new());
        }
    }

    if let (Some(env_path), Some(lines)) = (env_file, env_lines) {
        let mut contents = String::new();
        contents.push_str("# Generated by contract tls stage\n");
        for line in lines {
            contents.push_str(&line);
            contents.push('\n');
        }
        maybe_write(&env_path, contents.as_bytes(), force)?;
        staged_paths.push(env_path);
    }

    Ok(staged_paths)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceTarget {
    label: String,
    mode: ClientAuthMode,
    directory: PathBuf,
    env_prefix: String,
}

struct ServiceManifest {
    version: u8,
    generated_at: String,
    service: String,
    directory: String,
    env_prefix: String,
    client_auth: String,
    staged_files: Vec<String>,
    env_exports: Vec<EnvExport>,
    renewal_timestamp: Option<String>,
    renewal_reminder: Option<String>,
    renewal_window_days: Option<u32>,
}

struct EnvExport {
    key: String,
    value: String,
}

impl FromStr for ServiceTarget {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (head, path) = input.split_once('=').ok_or_else(|| {
            format!("invalid service spec '{input}'; expected label[:mode]=/path")
        })?;
        if path.trim().is_empty() {
            return Err(format!(
                "service spec '{input}' must provide a target directory"
            ));
        }
        let (label_mode, env_override) = if let Some((left, env)) = head.split_once('@') {
            (left, Some(env.trim()))
        } else {
            (head, None)
        };
        let (label, mode) = if let Some((label, mode)) = label_mode.split_once(':') {
            (label.trim(), mode.trim())
        } else {
            (label_mode.trim(), "none")
        };
        if label.is_empty() {
            return Err(format!("service spec '{input}' must include a label"));
        }
        let mode = ClientAuthMode::from_str(mode)?;
        let env_prefix = if let Some(override_prefix) = env_override {
            if override_prefix.is_empty() {
                return Err(format!(
                    "service spec '{input}' must provide a non-empty env prefix"
                ));
            }
            override_prefix.to_string()
        } else {
            default_env_prefix(label)
        };
        Ok(ServiceTarget {
            label: label.to_string(),
            mode,
            directory: PathBuf::from(path.trim()),
            env_prefix,
        })
    }
}

impl Serialize for ServiceManifest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ServiceManifest", 11)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("generated_at", &self.generated_at)?;
        state.serialize_field("service", &self.service)?;
        state.serialize_field("directory", &self.directory)?;
        state.serialize_field("env_prefix", &self.env_prefix)?;
        state.serialize_field("client_auth", &self.client_auth)?;
        state.serialize_field("staged_files", &self.staged_files)?;
        state.serialize_field("env_exports", &self.env_exports)?;
        if let Some(timestamp) = &self.renewal_timestamp {
            state.serialize_field("renewal_timestamp", timestamp)?;
        }
        if let Some(reminder) = &self.renewal_reminder {
            state.serialize_field("renewal_reminder", reminder)?;
        }
        if let Some(days) = &self.renewal_window_days {
            state.serialize_field("renewal_window_days", days)?;
        }
        state.end()
    }
}

impl Serialize for EnvExport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("EnvExport", 2)?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("value", &self.value)?;
        state.end()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientAuthMode {
    None,
    Required,
    Optional,
}

impl ClientAuthMode {
    fn requires_anchor(self) -> bool {
        matches!(self, ClientAuthMode::Required | ClientAuthMode::Optional)
    }

    fn as_str(self) -> &'static str {
        match self {
            ClientAuthMode::None => "none",
            ClientAuthMode::Required => "required",
            ClientAuthMode::Optional => "optional",
        }
    }
}

fn default_env_prefix(label: &str) -> String {
    let mut prefix = String::from("TB_");
    let mut last_was_underscore = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            prefix.push(ch.to_ascii_uppercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            prefix.push('_');
            last_was_underscore = true;
        }
    }
    while prefix.ends_with('_') {
        prefix.pop();
    }
    if !prefix.ends_with("_TLS") {
        if !prefix.ends_with('_') {
            prefix.push('_');
        }
        prefix.push_str("TLS");
    }
    prefix
}

impl FromStr for ClientAuthMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_ascii_lowercase().as_str() {
            "" | "none" => Ok(ClientAuthMode::None),
            "required" => Ok(ClientAuthMode::Required),
            "optional" => Ok(ClientAuthMode::Optional),
            other => Err(format!(
                "unknown client auth mode '{other}'; use none, required, or optional"
            )),
        }
    }
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

fn canonical_path(path: &Path) -> String {
    fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn parse_certificate(bytes: &[u8]) -> Result<[u8; 32], ConvertError> {
    if looks_like_json(bytes) {
        let value: Value =
            json::from_slice(bytes).map_err(|err| ConvertError::Encoding(err.to_string()))?;
        let map = value
            .as_object()
            .ok_or(ConvertError::invalid("certificate must be an object"))?;
        let algorithm = map
            .get("algorithm")
            .and_then(Value::as_str)
            .ok_or(ConvertError::invalid("certificate missing algorithm"))?;
        if !algorithm.eq_ignore_ascii_case("ed25519") {
            return Err(ConvertError::invalid(
                "certificate algorithm must be ed25519",
            ));
        }
        let public_key = map
            .get("public_key")
            .and_then(Value::as_str)
            .ok_or(ConvertError::invalid("certificate missing public_key"))?;
        let bytes = base64_fp::decode_standard(public_key)
            .map_err(|err| ConvertError::Encoding(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(ConvertError::invalid("certificate public key length"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }

    let ders = decode_der_blobs(bytes)?;
    let mut algorithms = Vec::new();
    for der in &ders {
        if let Ok(key) = ed25519_public_key_from_der(&der) {
            return Ok(key);
        }
        if let Some(label) = detect_public_key_algorithm(&der) {
            if label == "ed25519" {
                continue;
            }
            if !algorithms.contains(&label) {
                algorithms.push(label);
            }
        }
    }
    if !algorithms.is_empty() {
        return Err(ConvertError::invalid(format!(
            "certificate chain does not include an ed25519 entry (found: {})",
            algorithms.join(", ")
        )));
    }
    Err(ConvertError::invalid(
        "certificate did not contain an ed25519 key",
    ))
}

fn parse_private_key(bytes: &[u8]) -> Result<[u8; 32], ConvertError> {
    if looks_like_json(bytes) {
        let value: Value =
            json::from_slice(bytes).map_err(|err| ConvertError::Encoding(err.to_string()))?;
        let map = value
            .as_object()
            .ok_or(ConvertError::invalid("private key must be an object"))?;
        let algorithm = map
            .get("algorithm")
            .and_then(Value::as_str)
            .ok_or(ConvertError::invalid("private key missing algorithm"))?;
        if !algorithm.eq_ignore_ascii_case("ed25519") {
            return Err(ConvertError::invalid(
                "private key algorithm must be ed25519",
            ));
        }
        let private_key =
            map.get("private_key")
                .and_then(Value::as_str)
                .ok_or(ConvertError::invalid(
                    "private key missing private_key field",
                ))?;
        let bytes = base64_fp::decode_standard(private_key)
            .map_err(|err| ConvertError::Encoding(err.to_string()))?;
        if bytes.len() != 32 {
            return Err(ConvertError::invalid("private key length"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }

    let ders = decode_der_blobs(bytes)?;
    let mut algorithms = Vec::new();
    for der in &ders {
        if let Ok(signing) = SigningKey::from_pkcs8_der(&der) {
            return Ok(signing.to_bytes());
        }
        if let Some(secret) = extract_ed25519_private_key(&der) {
            return Ok(secret);
        }
        if let Some(label) = detect_public_key_algorithm(&der) {
            if label != "ed25519" && !algorithms.contains(&label) {
                algorithms.push(label);
            }
        }
    }
    if !algorithms.is_empty() {
        return Err(ConvertError::invalid(format!(
            "private key material is not ed25519 (found: {})",
            algorithms.join(", ")
        )));
    }
    Err(ConvertError::invalid(
        "private key did not contain an ed25519 key",
    ))
}

struct CertificateEntry {
    version: u8,
    algorithm: &'static str,
    public_key: String,
    not_after: Option<String>,
}

impl Serialize for CertificateEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = if self.not_after.is_some() { 4 } else { 3 };
        let mut state = serializer.serialize_struct("CertificateEntry", field_count)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("algorithm", &self.algorithm)?;
        state.serialize_field("public_key", &self.public_key)?;
        if let Some(not_after) = &self.not_after {
            state.serialize_field("not_after", not_after)?;
        }
        state.end()
    }
}

fn render_certificate_json(
    verifying: &[u8; 32],
    not_after: Option<UtcDateTime>,
) -> Result<Vec<u8>, ConvertError> {
    let encoded = base64_fp::encode_standard(verifying);
    let entry = CertificateEntry {
        version: 1,
        algorithm: "ed25519",
        public_key: encoded,
        not_after: match not_after {
            Some(ts) => Some(format_timestamp(ts)?),
            None => None,
        },
    };
    json::to_vec(&entry).map_err(|err| ConvertError::Encoding(err.to_string()))
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

fn format_timestamp(timestamp: UtcDateTime) -> Result<String, ConvertError> {
    timestamp
        .format_iso8601()
        .map_err(|_| ConvertError::invalid("failed to encode timestamp"))
}

fn parse_iso8601_timestamp(value: &str) -> Result<UtcDateTime, ConvertError> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return Err(ConvertError::invalid("timestamp must be ISO-8601"));
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'Z' {
        return Err(ConvertError::invalid("timestamp must be ISO-8601"));
    }
    let year = parse_decimal(&bytes[0..4])? as i32;
    let month = parse_decimal(&bytes[5..7])? as u8;
    let day = parse_decimal(&bytes[8..10])? as u8;
    let hour = parse_decimal(&bytes[11..13])? as u8;
    let minute = parse_decimal(&bytes[14..16])? as u8;
    let second = parse_decimal(&bytes[17..19])? as u8;
    timestamp_from_components(year, month, day, hour, minute, second)
}

fn extract_certificate_not_after(bytes: &[u8]) -> Result<Option<UtcDateTime>, ConvertError> {
    if looks_like_json(bytes) {
        if let Ok(Value::Object(map)) = json::from_slice::<Value>(bytes) {
            if let Some(Value::String(value)) = map.get("not_after") {
                return parse_iso8601_timestamp(value).map(Some);
            }
        }
        return Ok(None);
    }

    let ders = decode_der_blobs(bytes)?;
    for der in &ders {
        if let Some(ts) = parse_not_after_from_der(der)? {
            return Ok(Some(ts));
        }
    }
    Ok(None)
}

fn parse_not_after_from_der(der: &[u8]) -> Result<Option<UtcDateTime>, ConvertError> {
    let mut root = DerReader::new(der);
    let certificate = root.read_element()?;
    if certificate.tag != 0x30 {
        return Err(ConvertError::invalid("certificate must be a sequence"));
    }
    let mut certificate_reader = DerReader::new(certificate.value);
    let tbs = certificate_reader.read_element()?;
    if tbs.tag != 0x30 {
        return Err(ConvertError::invalid("tbsCertificate must be a sequence"));
    }
    let mut tbs_reader = DerReader::new(tbs.value);
    tbs_reader.read_if_context_specific(0)?; // version
    tbs_reader.read_element()?; // serial
    tbs_reader.read_element()?; // signature
    tbs_reader.read_element()?; // issuer
    let validity = tbs_reader.read_element()?;
    if validity.tag != 0x30 {
        return Err(ConvertError::invalid("validity must be a sequence"));
    }
    let mut validity_reader = DerReader::new(validity.value);
    let _not_before = validity_reader.read_element()?;
    let not_after = validity_reader
        .read_element()
        .map_err(|_| ConvertError::invalid("certificate missing notAfter"))?;
    Ok(Some(parse_der_time(&not_after)?))
}

fn parse_der_time(element: &DerElement<'_>) -> Result<UtcDateTime, ConvertError> {
    match element.tag {
        0x17 => parse_utc_time(element.value),
        0x18 => parse_generalized_time(element.value),
        _ => Err(ConvertError::invalid("unexpected time tag")),
    }
}

fn parse_utc_time(bytes: &[u8]) -> Result<UtcDateTime, ConvertError> {
    if bytes.len() != 13 || bytes[12] != b'Z' {
        return Err(ConvertError::invalid("unsupported UTCTime format"));
    }
    let year = parse_decimal(&bytes[0..2])? as i32;
    let year = if year >= 50 { 1900 + year } else { 2000 + year };
    let month = parse_decimal(&bytes[2..4])? as u8;
    let day = parse_decimal(&bytes[4..6])? as u8;
    let hour = parse_decimal(&bytes[6..8])? as u8;
    let minute = parse_decimal(&bytes[8..10])? as u8;
    let second = parse_decimal(&bytes[10..12])? as u8;
    timestamp_from_components(year, month, day, hour, minute, second)
}

fn parse_generalized_time(bytes: &[u8]) -> Result<UtcDateTime, ConvertError> {
    if bytes.len() != 15 || bytes[14] != b'Z' {
        return Err(ConvertError::invalid("unsupported GeneralizedTime format"));
    }
    let year = parse_decimal(&bytes[0..4])? as i32;
    let month = parse_decimal(&bytes[4..6])? as u8;
    let day = parse_decimal(&bytes[6..8])? as u8;
    let hour = parse_decimal(&bytes[8..10])? as u8;
    let minute = parse_decimal(&bytes[10..12])? as u8;
    let second = parse_decimal(&bytes[12..14])? as u8;
    timestamp_from_components(year, month, day, hour, minute, second)
}

fn timestamp_from_components(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> Result<UtcDateTime, ConvertError> {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(ConvertError::invalid("invalid timestamp components"));
    }
    let days = days_from_civil(year, month, day);
    let mut seconds = days as i128 * 86_400;
    seconds += hour as i128 * 3_600;
    seconds += minute as i128 * 60;
    seconds += second as i128;
    if seconds < i64::MIN as i128 || seconds > i64::MAX as i128 {
        return Err(ConvertError::invalid("timestamp out of range"));
    }
    UtcDateTime::from_unix_timestamp(seconds as i64)
        .map_err(|_| ConvertError::invalid("timestamp out of range"))
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = year as i64;
    let month = month as i64;
    let day = day as i64;
    let y = year - if month <= 2 { 1 } else { 0 };
    let m = month + if month <= 2 { 9 } else { -3 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719_468
}

fn parse_decimal(bytes: &[u8]) -> Result<u32, ConvertError> {
    let mut value = 0u32;
    if bytes.is_empty() {
        return Err(ConvertError::invalid("expected digits"));
    }
    for &b in bytes {
        if !(b'0'..=b'9').contains(&b) {
            return Err(ConvertError::invalid("expected digits"));
        }
        value = value * 10 + (b - b'0') as u32;
    }
    Ok(value)
}

fn render_manifest_yaml(manifest: &ServiceManifest) -> String {
    let mut out = String::new();
    out.push_str("version: 1\n");
    out.push_str(&format!(
        "generated_at: {}\n",
        yaml_escape(&manifest.generated_at)
    ));
    out.push_str(&format!("service: {}\n", yaml_escape(&manifest.service)));
    out.push_str(&format!(
        "directory: {}\n",
        yaml_escape(&manifest.directory)
    ));
    out.push_str(&format!(
        "env_prefix: {}\n",
        yaml_escape(&manifest.env_prefix)
    ));
    out.push_str(&format!(
        "client_auth: {}\n",
        yaml_escape(&manifest.client_auth)
    ));
    if manifest.staged_files.is_empty() {
        out.push_str("staged_files: []\n");
    } else {
        out.push_str("staged_files:\n");
        for file in &manifest.staged_files {
            out.push_str("  - ");
            out.push_str(&yaml_escape(file));
            out.push('\n');
        }
    }
    if manifest.env_exports.is_empty() {
        out.push_str("env_exports: []\n");
    } else {
        out.push_str("env_exports:\n");
        for export in &manifest.env_exports {
            out.push_str("  - key: ");
            out.push_str(&yaml_escape(&export.key));
            out.push('\n');
            out.push_str("    value: ");
            out.push_str(&yaml_escape(&export.value));
            out.push('\n');
        }
    }
    if let Some(ref ts) = manifest.renewal_timestamp {
        out.push_str(&format!("renewal_timestamp: {}\n", yaml_escape(ts)));
    }
    if let Some(ref ts) = manifest.renewal_reminder {
        out.push_str(&format!("renewal_reminder: {}\n", yaml_escape(ts)));
    }
    if let Some(days) = manifest.renewal_window_days {
        out.push_str(&format!("renewal_window_days: {}\n", days));
    }
    out
}

fn yaml_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

struct DerReader<'a> {
    data: &'a [u8],
    offset: usize,
}

struct DerElement<'a> {
    tag: u8,
    _constructed: bool,
    value: &'a [u8],
}

impl<'a> DerReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn read_element(&mut self) -> Result<DerElement<'a>, ConvertError> {
        if self.offset >= self.data.len() {
            return Err(ConvertError::invalid("unexpected end of der"));
        }
        let tag = self.data[self.offset];
        self.offset += 1;
        if self.offset >= self.data.len() {
            return Err(ConvertError::invalid("invalid der length"));
        }
        let len_byte = self.data[self.offset];
        self.offset += 1;
        let length = if len_byte & 0x80 == 0 {
            len_byte as usize
        } else {
            let octets = (len_byte & 0x7F) as usize;
            if octets == 0 || octets > 4 {
                return Err(ConvertError::invalid("unsupported der length"));
            }
            if self.offset + octets > self.data.len() {
                return Err(ConvertError::invalid("invalid der length"));
            }
            let mut value_len = 0usize;
            for _ in 0..octets {
                value_len = (value_len << 8) | self.data[self.offset] as usize;
                self.offset += 1;
            }
            value_len
        };
        if self.offset + length > self.data.len() {
            return Err(ConvertError::invalid("invalid der payload"));
        }
        let value = &self.data[self.offset..self.offset + length];
        self.offset += length;
        Ok(DerElement {
            tag,
            _constructed: tag & 0x20 != 0,
            value,
        })
    }

    fn read_if_context_specific(
        &mut self,
        tag_number: u8,
    ) -> Result<Option<DerElement<'a>>, ConvertError> {
        if self.offset >= self.data.len() {
            return Ok(None);
        }
        let saved = self.offset;
        let element = self.read_element()?;
        let expected = 0xA0 + tag_number;
        if element.tag == expected {
            Ok(Some(element))
        } else {
            self.offset = saved;
            Ok(None)
        }
    }
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
                return Err(ConvertError::invalid("nested pem begin"));
            }
            if let Some(end) = rest.strip_suffix("-----") {
                current_label = Some(end.trim().to_string());
                buffer.clear();
                continue;
            }
            return Err(ConvertError::invalid("invalid pem begin"));
        }
        if let Some(rest) = line.strip_prefix("-----END ") {
            let label = current_label
                .take()
                .ok_or(ConvertError::invalid("pem end without begin"))?;
            if !rest.starts_with(&label) {
                return Err(ConvertError::invalid("pem end label mismatch"));
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
        return Err(ConvertError::invalid("unterminated pem block"));
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

fn detect_public_key_algorithm(der: &[u8]) -> Option<&'static str> {
    const OIDS: [(&[u8], &str); 6] = [
        (&[0x06, 0x03, 0x2B, 0x65, 0x70], "ed25519"),
        (&[0x06, 0x03, 0x2B, 0x65, 0x6E], "x25519"),
        (
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
            ],
            "rsa",
        ),
        (
            &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01],
            "ec",
        ),
        (
            &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07],
            "prime256v1",
        ),
        (&[0x06, 0x03, 0x2B, 0x65, 0x71], "ed448"),
    ];

    for (oid, label) in OIDS {
        if der.windows(oid.len()).any(|window| window == oid) {
            return Some(label);
        }
    }
    None
}

fn extract_ed25519_private_key(der: &[u8]) -> Option<[u8; 32]> {
    let mut cursor = 0;
    let sequence = read_tlv(der, &mut cursor, 0x30)?;
    let mut seq_cursor = 0;
    let _version = read_tlv(sequence, &mut seq_cursor, 0x02)?;
    let algorithm = read_tlv(sequence, &mut seq_cursor, 0x30)?;
    let mut algo_cursor = 0;
    let oid = read_tlv(algorithm, &mut algo_cursor, 0x06)?;
    if oid != [0x2B, 0x65, 0x70] {
        return None;
    }
    let private_key_octets = read_tlv(sequence, &mut seq_cursor, 0x04)?;
    decode_private_key_octets(private_key_octets)
}

fn decode_private_key_octets(bytes: &[u8]) -> Option<[u8; 32]> {
    if bytes.len() == 32 {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(bytes);
        return Some(secret);
    }
    let mut cursor = 0;
    let inner = read_tlv(bytes, &mut cursor, 0x04)?;
    if inner.len() == 32 {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(inner);
        return Some(secret);
    }
    None
}

fn read_tlv<'a>(bytes: &'a [u8], cursor: &mut usize, tag: u8) -> Option<&'a [u8]> {
    let &actual = bytes.get(*cursor)?;
    if actual != tag {
        return None;
    }
    *cursor += 1;
    let len = read_length(bytes, cursor)?;
    if bytes.len() < *cursor + len {
        return None;
    }
    let start = *cursor;
    *cursor += len;
    Some(&bytes[start..start + len])
}

fn read_length(bytes: &[u8], cursor: &mut usize) -> Option<usize> {
    let first = *bytes.get(*cursor)?;
    *cursor += 1;
    if first & 0x80 == 0 {
        return Some(first as usize);
    }
    let count = (first & 0x7F) as usize;
    if count == 0 || count > bytes.len().saturating_sub(*cursor) {
        return None;
    }
    let mut value = 0usize;
    for _ in 0..count {
        value = (value << 8) | (*bytes.get(*cursor)? as usize);
        *cursor += 1;
    }
    Some(value)
}

fn required_option(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_string(name)
        .ok_or_else(|| format!("missing required option '--{name}'"))
}

#[derive(Debug, Clone, PartialEq)]
struct CliTlsWarningStatus {
    retention_seconds: u64,
    active_snapshots: usize,
    stale_snapshots: usize,
    most_recent_last_seen: Option<u64>,
    least_recent_last_seen: Option<u64>,
}

impl Serialize for CliTlsWarningStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CliTlsWarningStatus", 5)?;
        state.serialize_field("retention_seconds", &self.retention_seconds)?;
        state.serialize_field("active_snapshots", &self.active_snapshots)?;
        state.serialize_field("stale_snapshots", &self.stale_snapshots)?;
        state.serialize_field("most_recent_last_seen", &self.most_recent_last_seen)?;
        state.serialize_field("least_recent_last_seen", &self.least_recent_last_seen)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CliTlsWarningStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let mut map = match value {
            Value::Object(map) => map,
            other => {
                return Err(de::Error::custom(format!(
                    "expected status object, found {other:?}"
                )))
            }
        };

        let retention_seconds =
            decode_required_field::<u64, D::Error>(&mut map, "retention_seconds")?;
        let active_snapshots =
            decode_required_field::<usize, D::Error>(&mut map, "active_snapshots")?;
        let stale_snapshots =
            decode_required_field::<usize, D::Error>(&mut map, "stale_snapshots")?;
        let most_recent_last_seen =
            decode_nullable_field::<u64, D::Error>(&mut map, "most_recent_last_seen")?;
        let least_recent_last_seen =
            decode_nullable_field::<u64, D::Error>(&mut map, "least_recent_last_seen")?;

        Ok(CliTlsWarningStatus {
            retention_seconds,
            active_snapshots,
            stale_snapshots,
            most_recent_last_seen,
            least_recent_last_seen,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CliTlsWarningSnapshot {
    prefix: String,
    code: String,
    total: u64,
    last_delta: u64,
    last_seen: u64,
    origin: CliTlsWarningOrigin,
    peer_id: Option<String>,
    detail: Option<String>,
    variables: Vec<String>,
}

impl Serialize for CliTlsWarningSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CliTlsWarningSnapshot", 9)?;
        state.serialize_field("prefix", &self.prefix)?;
        state.serialize_field("code", &self.code)?;
        state.serialize_field("total", &self.total)?;
        state.serialize_field("last_delta", &self.last_delta)?;
        state.serialize_field("last_seen", &self.last_seen)?;
        state.serialize_field("origin", &self.origin)?;
        state.serialize_field("peer_id", &self.peer_id)?;
        state.serialize_field("detail", &self.detail)?;
        state.serialize_field("variables", &self.variables)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CliTlsWarningSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let mut map = match value {
            Value::Object(map) => map,
            other => {
                return Err(de::Error::custom(format!(
                    "expected snapshot object, found {other:?}"
                )))
            }
        };

        let prefix = decode_required_field::<String, D::Error>(&mut map, "prefix")?;
        let code = decode_required_field::<String, D::Error>(&mut map, "code")?;
        let total = decode_required_field::<u64, D::Error>(&mut map, "total")?;
        let last_delta = decode_required_field::<u64, D::Error>(&mut map, "last_delta")?;
        let last_seen = decode_required_field::<u64, D::Error>(&mut map, "last_seen")?;
        let origin = decode_required_field::<CliTlsWarningOrigin, D::Error>(&mut map, "origin")?;
        let peer_id = decode_nullable_field::<String, D::Error>(&mut map, "peer_id")?;
        let detail = decode_nullable_field::<String, D::Error>(&mut map, "detail")?;
        let variables = decode_required_field::<Vec<String>, D::Error>(&mut map, "variables")?;

        Ok(CliTlsWarningSnapshot {
            prefix,
            code,
            total,
            last_delta,
            last_seen,
            origin,
            peer_id,
            detail,
            variables,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum CliTlsWarningOrigin {
    Diagnostics,
    PeerIngest,
}

impl Serialize for CliTlsWarningOrigin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            CliTlsWarningOrigin::Diagnostics => serializer.serialize_str("diagnostics"),
            CliTlsWarningOrigin::PeerIngest => serializer.serialize_str("peer_ingest"),
        }
    }
}

impl<'de> Deserialize<'de> for CliTlsWarningOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OriginVisitor;

        impl<'de> Visitor<'de> for OriginVisitor {
            type Value = CliTlsWarningOrigin;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("diagnostics or peer_ingest")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "diagnostics" => Ok(CliTlsWarningOrigin::Diagnostics),
                    "peer_ingest" => Ok(CliTlsWarningOrigin::PeerIngest),
                    other => Err(E::unknown_variant(other, &["diagnostics", "peer_ingest"])),
                }
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_str(OriginVisitor)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CliTlsStatusReport {
    aggregator: String,
    status: CliTlsWarningStatus,
    snapshots: Option<Vec<CliTlsWarningSnapshot>>,
    suggestions: Vec<String>,
    generated_at: u64,
}

impl Serialize for CliTlsStatusReport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = if self.snapshots.is_some() { 5 } else { 4 };
        let mut state = serializer.serialize_struct("CliTlsStatusReport", field_count)?;
        state.serialize_field("aggregator", &self.aggregator)?;
        state.serialize_field("status", &self.status)?;
        if let Some(snapshots) = &self.snapshots {
            state.serialize_field("snapshots", snapshots)?;
        }
        state.serialize_field("suggestions", &self.suggestions)?;
        state.serialize_field("generated_at", &self.generated_at)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CliTlsStatusReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let mut map = match value {
            Value::Object(map) => map,
            other => {
                return Err(de::Error::custom(format!(
                    "expected status report object, found {other:?}"
                )))
            }
        };

        let aggregator = decode_required_field::<String, D::Error>(&mut map, "aggregator")?;
        let status = decode_required_field::<CliTlsWarningStatus, D::Error>(&mut map, "status")?;
        let snapshots =
            decode_nullable_field::<Vec<CliTlsWarningSnapshot>, D::Error>(&mut map, "snapshots")?;
        let suggestions = decode_required_field::<Vec<String>, D::Error>(&mut map, "suggestions")?;
        let generated_at = decode_required_field::<u64, D::Error>(&mut map, "generated_at")?;

        Ok(CliTlsStatusReport {
            aggregator,
            status,
            snapshots,
            suggestions,
            generated_at,
        })
    }
}

fn decode_required_field<T, E>(map: &mut JsonMap, key: &'static str) -> Result<T, E>
where
    T: DeserializeOwned,
    E: de::Error,
{
    let value = map.remove(key).ok_or_else(|| E::missing_field(key))?;
    json::from_value(value).map_err(|err| E::custom(format!("failed to decode '{key}': {err}")))
}

fn decode_nullable_field<T, E>(map: &mut JsonMap, key: &'static str) -> Result<Option<T>, E>
where
    T: DeserializeOwned,
    E: de::Error,
{
    match map.remove(key) {
        Some(value) => json::from_value::<Option<T>>(value)
            .map_err(|err| E::custom(format!("failed to decode '{key}': {err}"))),
        None => Ok(None),
    }
}

fn status_tls(
    aggregator: String,
    include_latest: bool,
    output: StatusOutput,
) -> Result<(), String> {
    let base = aggregator.trim_end_matches('/').to_string();
    let status = fetch_tls_status(&base)?;
    let snapshots = if include_latest {
        Some(fetch_tls_latest(&base)?)
    } else {
        None
    };
    let now = current_unix_timestamp();
    let suggestions =
        generate_status_suggestions(&status, snapshots.as_deref(), now, include_latest);

    if matches!(output, StatusOutput::Json) {
        let report = CliTlsStatusReport {
            aggregator: base.clone(),
            status,
            snapshots,
            suggestions,
            generated_at: now,
        };
        let json = json::to_vec_pretty(&report)
            .map_err(|err| format!("failed to encode status report: {err}"))?;
        println!("{}", String::from_utf8_lossy(&json));
        return Ok(());
    }

    let rendered = render_status_report(&base, &status, snapshots.as_deref(), now, &suggestions)?;
    print!("{}", rendered);
    Ok(())
}

fn fetch_tls_status(base: &str) -> Result<CliTlsWarningStatus, String> {
    let client = http_client::blocking_client();
    let url = format!("{}/tls/warnings/status", base);
    let response = client
        .request(Method::Get, &url)
        .map_err(|err| format!("failed to construct status request: {err}"))?
        .send()
        .map_err(|err| format!("failed to query {url}: {err}"))?;
    let status_code = response.status();
    if !status_code.is_success() {
        return Err(format!(
            "aggregator responded with status {} for {}",
            status_code.as_u16(),
            url
        ));
    }
    response
        .json::<CliTlsWarningStatus>()
        .map_err(|err| format!("failed to decode status payload: {err}"))
}

fn fetch_tls_latest(base: &str) -> Result<Vec<CliTlsWarningSnapshot>, String> {
    let client = http_client::blocking_client();
    let url = format!("{}/tls/warnings/latest", base);
    let response = client
        .request(Method::Get, &url)
        .map_err(|err| format!("failed to construct latest request: {err}"))?
        .send()
        .map_err(|err| format!("failed to query {url}: {err}"))?;
    let status_code = response.status();
    if !status_code.is_success() {
        return Err(format!(
            "aggregator responded with status {} for {}",
            status_code.as_u16(),
            url
        ));
    }
    response
        .json::<Vec<CliTlsWarningSnapshot>>()
        .map_err(|err| format!("failed to decode snapshot payload: {err}"))
}

fn render_status_report(
    aggregator: &str,
    status: &CliTlsWarningStatus,
    snapshots: Option<&[CliTlsWarningSnapshot]>,
    now: u64,
    suggestions: &[String],
) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "TLS warning status for {}", aggregator).map_err(stringify_fmt_error)?;
    let retention_label = if status.retention_seconds == 0 {
        "disabled".to_string()
    } else {
        format_duration(status.retention_seconds)
    };
    writeln!(
        out,
        "Retention window: {} ({})",
        status.retention_seconds, retention_label
    )
    .map_err(stringify_fmt_error)?;
    writeln!(out, "Active snapshots: {}", status.active_snapshots).map_err(stringify_fmt_error)?;
    writeln!(out, "Stale snapshots: {}", status.stale_snapshots).map_err(stringify_fmt_error)?;

    if let Some(ts) = status.most_recent_last_seen {
        let age = now.saturating_sub(ts);
        let stamp = format_unix_timestamp(ts);
        writeln!(
            out,
            "Most recent last_seen: {} (age {})",
            stamp,
            format_duration(age)
        )
        .map_err(stringify_fmt_error)?;
    } else {
        writeln!(out, "Most recent last_seen: n/a").map_err(stringify_fmt_error)?;
    }

    if let Some(ts) = status.least_recent_last_seen {
        let age = now.saturating_sub(ts);
        let stamp = format_unix_timestamp(ts);
        writeln!(
            out,
            "Oldest last_seen: {} (age {})",
            stamp,
            format_duration(age)
        )
        .map_err(stringify_fmt_error)?;
    } else {
        writeln!(out, "Oldest last_seen: n/a").map_err(stringify_fmt_error)?;
    }

    match snapshots {
        Some(entries) if entries.is_empty() => {
            writeln!(out, "\nSnapshots: none reported").map_err(stringify_fmt_error)?;
        }
        Some(entries) => {
            writeln!(out, "\nSnapshots:").map_err(stringify_fmt_error)?;
            for entry in entries {
                writeln!(
                    out,
                    "- {} · {} (origin: {}, total: {}, last_delta: {}, peer: {})",
                    entry.prefix,
                    entry.code,
                    origin_label(&entry.origin),
                    entry.total,
                    entry.last_delta,
                    entry.peer_id.as_deref().unwrap_or("n/a")
                )
                .map_err(stringify_fmt_error)?;
                let stamp = format_unix_timestamp(entry.last_seen);
                let age = now.saturating_sub(entry.last_seen);
                let stale_note = match stale_over(entry, status, now) {
                    Some(over) => format!(" [STALE +{}]", format_duration(over)),
                    None => String::new(),
                };
                writeln!(
                    out,
                    "  last_seen: {} (age {}{})",
                    stamp,
                    format_duration(age),
                    stale_note
                )
                .map_err(stringify_fmt_error)?;
                if let Some(detail) = entry.detail.as_ref().filter(|d| !d.is_empty()) {
                    writeln!(out, "  detail: {}", detail).map_err(stringify_fmt_error)?;
                }
                if !entry.variables.is_empty() {
                    writeln!(out, "  variables: {}", entry.variables.join(", "))
                        .map_err(stringify_fmt_error)?;
                }
            }
        }
        None if status.active_snapshots > 0 => {
            writeln!(
                out,
                "\nSnapshots: run 'contract tls status --latest' to list {} tracked entries",
                status.active_snapshots
            )
            .map_err(stringify_fmt_error)?;
        }
        _ => {}
    }

    if !suggestions.is_empty() {
        writeln!(out, "\nSuggested actions:").map_err(stringify_fmt_error)?;
        for suggestion in suggestions {
            writeln!(out, "  - {}", suggestion).map_err(stringify_fmt_error)?;
        }
    }

    Ok(out)
}

fn stringify_fmt_error(err: std::fmt::Error) -> String {
    err.to_string()
}

fn generate_status_suggestions(
    status: &CliTlsWarningStatus,
    snapshots: Option<&[CliTlsWarningSnapshot]>,
    now: u64,
    include_latest: bool,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    if status.retention_seconds == 0 {
        suggestions.push(
            "Retention is disabled; set AGGREGATOR_TLS_WARNING_RETENTION_SECS to retain history."
                .into(),
        );
    }
    if status.stale_snapshots > 0 {
        if let Some(entries) = snapshots {
            let offenders: Vec<String> = entries
                .iter()
                .filter(|entry| stale_over(entry, status, now).is_some())
                .map(|entry| format!("{}·{}", entry.prefix, entry.code))
                .take(5)
                .collect();
            if !offenders.is_empty() {
                suggestions.push(format!(
                    "Rotate TLS materials or prune manifests for stale snapshots: {}.",
                    offenders.join(", ")
                ));
            } else {
                suggestions.push(
                    "Stale snapshots detected; inspect manifests and rotation pipelines for drift."
                        .into(),
                );
            }
        } else {
            suggestions.push(format!(
                "{} snapshot(s) exceed the retention window; rerun with '--latest' to identify prefixes.",
                status.stale_snapshots
            ));
        }
    }
    if status.active_snapshots == 0 {
        suggestions.push(
            "No TLS warnings are currently tracked; verify recent rotations completed cleanly."
                .into(),
        );
    } else if status.stale_snapshots == 0 {
        suggestions.push("All tracked warnings are within the retention window; continue monitoring dashboards for new events.".into());
    }
    if include_latest {
        if let Some(entries) = snapshots {
            if entries.is_empty() {
                suggestions.push(
                    "Latest snapshot list is empty; confirm the aggregator is exporting TLS gauges.".into(),
                );
            }
        }
    } else if status.active_snapshots > 0 {
        suggestions
            .push("Use '--latest' to inspect per-prefix details when triaging warnings.".into());
    }
    if status.retention_seconds > 0 && status.retention_seconds < 86_400 {
        suggestions.push("Consider widening AGGREGATOR_TLS_WARNING_RETENTION_SECS if rotations span more than a day.".into());
    }
    suggestions
}

fn stale_over(
    snapshot: &CliTlsWarningSnapshot,
    status: &CliTlsWarningStatus,
    now: u64,
) -> Option<u64> {
    if status.retention_seconds == 0 {
        return None;
    }
    let age = now.saturating_sub(snapshot.last_seen);
    if age > status.retention_seconds {
        Some(age - status.retention_seconds)
    } else {
        None
    }
}

fn origin_label(origin: &CliTlsWarningOrigin) -> &'static str {
    match origin {
        CliTlsWarningOrigin::Diagnostics => "diagnostics",
        CliTlsWarningOrigin::PeerIngest => "peer_ingest",
    }
}

fn format_unix_timestamp(ts: u64) -> String {
    if let Ok(dt) = UtcDateTime::from_unix_timestamp(ts as i64) {
        if let Ok(text) = dt.format_iso8601() {
            return text;
        }
    }
    ts.to_string()
}

fn format_duration(seconds: u64) -> String {
    if seconds == 0 {
        return "0s".to_string();
    }
    let mut remaining = seconds;
    let mut parts = Vec::new();
    let days = remaining / 86_400;
    if days > 0 {
        parts.push(format!("{}d", days));
        remaining %= 86_400;
    }
    let hours = remaining / 3_600;
    if hours > 0 {
        parts.push(format!("{}h", hours));
        remaining %= 3_600;
    }
    let minutes = remaining / 60;
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
        remaining %= 60;
    }
    if remaining > 0 {
        parts.push(format!("{}s", remaining));
    }
    parts.join(" ")
}

fn current_unix_timestamp() -> u64 {
    let secs = UtcDateTime::now().unix_timestamp().unwrap_or(0);
    if secs < 0 {
        0
    } else {
        secs as u64
    }
}

struct AnchorEntry<'a> {
    version: u8,
    allowed: Vec<AnchorIdentity<'a>>,
}

struct AnchorIdentity<'a> {
    algorithm: &'a str,
    public_key: String,
}

impl<'a> Serialize for AnchorEntry<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AnchorEntry", 2)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("allowed", &self.allowed)?;
        state.end()
    }
}

impl<'a> Serialize for AnchorIdentity<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AnchorIdentity", 2)?;
        state.serialize_field("algorithm", &self.algorithm)?;
        state.serialize_field("public_key", &self.public_key)?;
        state.end()
    }
}

#[derive(Debug)]
enum ConvertError {
    Io(io::Error),
    Invalid(Cow<'static, str>),
    Encoding(String),
}

impl From<io::Error> for ConvertError {
    fn from(value: io::Error) -> Self {
        ConvertError::Io(value)
    }
}

impl ConvertError {
    fn invalid(message: impl Into<Cow<'static, str>>) -> Self {
        ConvertError::Invalid(message.into())
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
    use foundation_serialization::json::{self, Value as JsonValue};
    use foundation_time::{Duration, UtcDateTime};
    use foundation_tls::{generate_self_signed_ed25519, SelfSignedCertParams};
    use rand::rngs::OsRng;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static STAGE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parse_certificate_json_round_trips() {
        let signing = SigningKey::generate(&mut OsRng::default());
        let verifying = signing.verifying_key();
        let json = render_certificate_json(&verifying.to_bytes(), None).expect("render cert");
        let parsed = parse_certificate(&json).expect("parse certificate");
        assert_eq!(parsed, verifying.to_bytes());
    }

    #[test]
    fn certificate_entry_omits_not_after_when_absent() {
        let entry = CertificateEntry {
            version: 1,
            algorithm: "ed25519",
            public_key: "base64".into(),
            not_after: None,
        };

        let rendered = json::to_value(&entry).expect("serialize entry");
        let object = rendered.as_object().expect("object value");
        assert!(object.get("not_after").is_none());
        assert_eq!(
            object.get("algorithm").and_then(JsonValue::as_str),
            Some("ed25519")
        );
    }

    #[test]
    fn tls_status_report_round_trips_through_json() {
        let status = CliTlsWarningStatus {
            retention_seconds: 600,
            active_snapshots: 2,
            stale_snapshots: 1,
            most_recent_last_seen: Some(1_000),
            least_recent_last_seen: None,
        };

        let snapshot = CliTlsWarningSnapshot {
            prefix: "TB_NODE_TLS".into(),
            code: "missing_anchor".into(),
            total: 4,
            last_delta: 2,
            last_seen: 995,
            origin: CliTlsWarningOrigin::PeerIngest,
            peer_id: Some("peer-b".into()),
            detail: Some("anchor missing".into()),
            variables: vec!["TB_NODE_CERT".into(), "TB_NODE_KEY".into()],
        };

        let report = CliTlsStatusReport {
            aggregator: "https://agg".into(),
            status: status.clone(),
            snapshots: Some(vec![snapshot.clone()]),
            suggestions: vec!["rotate anchors".into()],
            generated_at: 1_111,
        };

        let encoded = json::to_vec(&report).expect("serialize report");
        let decoded: CliTlsStatusReport = json::from_slice(&encoded).expect("deserialize report");
        assert_eq!(decoded, report);
        assert_eq!(decoded.snapshots.unwrap()[0], snapshot);
    }

    #[test]
    fn tls_snapshot_deserializer_ignores_unknown_fields() {
        let mut payload_map = JsonMap::new();
        payload_map.insert("prefix".into(), Value::String("TB_GATEWAY_TLS".into()));
        payload_map.insert("code".into(), Value::String("expired_certificate".into()));
        payload_map.insert("total".into(), Value::Number(3.into()));
        payload_map.insert("last_delta".into(), Value::Number(1.into()));
        payload_map.insert("last_seen".into(), Value::Number(88.into()));
        payload_map.insert("origin".into(), Value::String("diagnostics".into()));
        payload_map.insert("peer_id".into(), Value::Null);
        payload_map.insert("detail".into(), Value::String("expired".into()));
        payload_map.insert(
            "variables".into(),
            Value::Array(vec![Value::String("TB_GATEWAY_CERT".into())]),
        );
        payload_map.insert("unexpected".into(), Value::String("value".into()));
        let payload = Value::Object(payload_map);

        let encoded = json::to_vec(&payload).expect("encode payload");
        let snapshot: CliTlsWarningSnapshot =
            json::from_slice(&encoded).expect("deserialize snapshot");
        assert_eq!(snapshot.prefix, "TB_GATEWAY_TLS");
        assert_eq!(snapshot.origin, CliTlsWarningOrigin::Diagnostics);
        assert_eq!(snapshot.variables, vec![String::from("TB_GATEWAY_CERT")]);
        assert!(snapshot.peer_id.is_none());
    }

    #[test]
    fn status_serialization_omits_optional_snapshots_field() {
        let report = CliTlsStatusReport {
            aggregator: "https://agg".into(),
            status: CliTlsWarningStatus {
                retention_seconds: 900,
                active_snapshots: 0,
                stale_snapshots: 0,
                most_recent_last_seen: None,
                least_recent_last_seen: None,
            },
            snapshots: None,
            suggestions: vec![],
            generated_at: 22,
        };

        let value = json::to_value(&report).expect("serialize");
        let object = value.as_object().expect("object");
        assert!(!object.contains_key("snapshots"));
        assert_eq!(
            object.get("aggregator").and_then(JsonValue::as_str),
            Some("https://agg")
        );
    }

    #[test]
    fn format_duration_compacts_units() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(3_661), "1h 1m 1s");
        assert_eq!(format_duration(90_061), "1d 1h 1m 1s");
    }

    #[test]
    fn render_status_report_marks_stale_entries() {
        let now = 200;
        let status = CliTlsWarningStatus {
            retention_seconds: 60,
            active_snapshots: 1,
            stale_snapshots: 1,
            most_recent_last_seen: Some(now - 10),
            least_recent_last_seen: Some(now - 120),
        };
        let snapshot = CliTlsWarningSnapshot {
            prefix: "TB_NODE_TLS".into(),
            code: "missing_anchor".into(),
            total: 2,
            last_delta: 1,
            last_seen: now - 120,
            origin: CliTlsWarningOrigin::Diagnostics,
            peer_id: Some("node-a".into()),
            detail: Some("anchors missing".into()),
            variables: vec!["TB_NODE_CERT".into()],
        };
        let report = render_status_report(
            "http://localhost:9000",
            &status,
            Some(&[snapshot]),
            now,
            &["rotate TLS materials".into()],
        )
        .expect("report renders");
        assert!(report.contains("TB_NODE_TLS · missing_anchor"));
        assert!(report.contains("[STALE +1m"));
        assert!(report.contains("rotate TLS materials"));
    }

    #[test]
    fn generate_status_suggestions_prompts_for_latest_when_missing() {
        let status = CliTlsWarningStatus {
            retention_seconds: 120,
            active_snapshots: 3,
            stale_snapshots: 2,
            most_recent_last_seen: Some(1_000),
            least_recent_last_seen: Some(800),
        };
        let suggestions = generate_status_suggestions(&status, None, 1_200, false);
        assert!(suggestions.iter().any(|entry| entry.contains("'--latest'")));
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

    #[test]
    fn pem_chain_prefers_first_ed25519_entry() {
        let (pem_chain, expected) = build_pem_chain();
        let parsed = parse_certificate(pem_chain.as_bytes()).expect("parse chain");
        assert_eq!(parsed, expected);
    }

    #[test]
    fn pem_chain_skips_non_ed25519_entries() {
        let (ed_pem, _, verifying) = build_pem_identity();
        let rsa_pem = include_str!("../../docs/assets/rsa_sample_cert.pem");
        let chain = format!("{rsa_pem}{ed_pem}");
        let parsed = parse_certificate(chain.as_bytes()).expect("parse chain");
        assert_eq!(parsed, verifying);
    }

    #[test]
    fn unsupported_certificate_reports_algorithm() {
        let pem = include_str!("../../docs/assets/rsa_sample_cert.pem");
        let err = parse_certificate(pem.as_bytes()).expect_err("rsa should fail");
        assert!(format!("{err}").contains("rsa"));
    }

    #[test]
    fn unsupported_private_key_reports_algorithm() {
        let pem = include_str!("../../docs/assets/rsa_sample_key.pem");
        let err = parse_private_key(pem.as_bytes()).expect_err("rsa key should fail");
        assert!(format!("{err}").contains("rsa"));
    }

    #[test]
    fn stage_tls_round_trips_into_loader() {
        let _guard = STAGE_LOCK.lock().unwrap();
        let (cert_pem, key_pem, _) = build_pem_identity();
        let input = temp_dir("tls_convert_input");
        let output = temp_dir("tls_stage_output");
        let cert_path = input.join("server.pem");
        let key_path = input.join("server.key");
        fs::write(&cert_path, &cert_pem).expect("write cert");
        fs::write(&key_path, &key_pem).expect("write key");
        let anchor_path = input.join("anchor.pem");
        fs::write(&anchor_path, &cert_pem).expect("write anchor");

        let convert_outputs = convert_tls(
            cert_path.clone(),
            key_path.clone(),
            Some(anchor_path.clone()),
            input.clone(),
            "identity".to_string(),
            true,
        )
        .expect("convert");
        assert!(!convert_outputs.is_empty());

        let cert_json_path = input.join("identity-cert.json");
        let cert_json = fs::read_to_string(&cert_json_path).expect("read cert json");
        let cert_value: Value = json::from_str(&cert_json).expect("parse cert json");
        let expected_not_after = cert_value
            .get("not_after")
            .and_then(Value::as_str)
            .map(str::to_string);
        assert!(expected_not_after.is_some(), "missing not_after metadata");

        let service_dir = output.join("aggregator");
        let spec = format!("aggregator:required={}", service_dir.display());
        let env_file = output.join("tls.env");
        let staged = stage_tls(
            input.clone(),
            "identity".to_string(),
            vec![spec.parse().unwrap()],
            true,
            Some(env_file.clone()),
        )
        .expect("stage");
        assert_eq!(staged.len(), 6);
        assert!(staged.contains(&env_file));

        let cert_dest = service_dir.join("cert.json");
        let key_dest = service_dir.join("key.json");
        let ca_dest = service_dir.join("client_ca.json");
        let manifest_json_path = service_dir.join("tls-manifest.json");
        let manifest_yaml_path = service_dir.join("tls-manifest.yaml");
        assert!(cert_dest.exists());
        assert!(key_dest.exists());
        assert!(ca_dest.exists());
        assert!(env_file.exists());
        assert!(manifest_json_path.exists());
        assert!(manifest_yaml_path.exists());
        assert!(staged.contains(&manifest_json_path));
        assert!(staged.contains(&manifest_yaml_path));

        let env_contents = fs::read_to_string(&env_file).expect("env file");
        let canonical_cert = canonical_path(&cert_dest);
        let canonical_key = canonical_path(&key_dest);
        let canonical_ca = canonical_path(&ca_dest);
        assert!(env_contents.contains(&format!("export TB_AGGREGATOR_TLS_CERT={canonical_cert}")));
        assert!(env_contents.contains(&format!("export TB_AGGREGATOR_TLS_KEY={canonical_key}")));
        assert!(env_contents.contains(&format!(
            "export TB_AGGREGATOR_TLS_CLIENT_CA={canonical_ca}"
        )));

        let manifest_json = fs::read(&manifest_json_path).expect("manifest json");
        let manifest_value: Value = json::from_slice(&manifest_json).expect("parse manifest");
        let manifest_map = manifest_value.as_object().expect("manifest map");
        assert_eq!(
            manifest_map.get("service").and_then(Value::as_str),
            Some("aggregator")
        );
        assert_eq!(
            manifest_map.get("client_auth").and_then(Value::as_str),
            Some("required")
        );
        if let Some(expected) = expected_not_after {
            assert_eq!(
                manifest_map
                    .get("renewal_timestamp")
                    .and_then(Value::as_str),
                Some(expected.as_str())
            );
        }
        let staged_files = manifest_map
            .get("staged_files")
            .and_then(Value::as_array)
            .expect("staged files array");
        assert!(staged_files
            .iter()
            .any(|v| v.as_str() == Some(canonical_cert.as_str())));
        assert!(staged_files
            .iter()
            .any(|v| v.as_str() == Some(canonical_key.as_str())));
        assert!(staged_files
            .iter()
            .any(|v| v.as_str() == Some(canonical_ca.as_str())));

        let manifest_yaml = fs::read_to_string(&manifest_yaml_path).expect("manifest yaml");
        assert!(manifest_yaml.contains("service: 'aggregator'"));
        assert!(manifest_yaml.contains("client_auth: 'required'"));

        std::env::set_var("TB_TEST_STAGE_CERT", cert_dest.display().to_string());
        std::env::set_var("TB_TEST_STAGE_KEY", key_dest.display().to_string());
        std::env::set_var("TB_TEST_STAGE_CLIENT_CA", ca_dest.display().to_string());

        let result = http_env::server_tls_from_env("TB_TEST_STAGE", None)
            .expect("load")
            .expect("loaded");
        assert_eq!(result.source_prefix, "TB_TEST_STAGE");

        std::env::remove_var("TB_TEST_STAGE_CERT");
        std::env::remove_var("TB_TEST_STAGE_KEY");
        std::env::remove_var("TB_TEST_STAGE_CLIENT_CA");
        fs::remove_dir_all(input).ok();
        fs::remove_dir_all(output).ok();
    }

    #[test]
    fn stage_tls_handles_optional_client_auth() {
        let _guard = STAGE_LOCK.lock().unwrap();
        let (cert_pem, key_pem, _) = build_pem_identity();
        let input = temp_dir("tls_optional_input");
        let output = temp_dir("tls_optional_output");
        let cert_path = input.join("server.pem");
        let key_path = input.join("server.key");
        fs::write(&cert_path, &cert_pem).expect("write cert");
        fs::write(&key_path, &key_pem).expect("write key");

        let anchor_path = input.join("anchor.pem");
        fs::write(&anchor_path, &cert_pem).expect("write anchor");

        convert_tls(
            cert_path.clone(),
            key_path.clone(),
            Some(anchor_path.clone()),
            input.clone(),
            "identity".to_string(),
            true,
        )
        .expect("convert");

        let service_dir = output.join("gateway");
        let env_file = output.join("optional.env");
        let spec = format!("gateway:optional={}", service_dir.display());
        let staged = stage_tls(
            input.clone(),
            "identity".to_string(),
            vec![spec.parse().unwrap()],
            true,
            Some(env_file.clone()),
        )
        .expect("stage optional");
        assert!(staged.contains(&service_dir.join("client_ca_optional.json")));
        assert!(staged.contains(&env_file));
        assert!(env_file.exists());
        let manifest_json_path = service_dir.join("tls-manifest.json");
        let manifest_yaml_path = service_dir.join("tls-manifest.yaml");
        assert!(manifest_json_path.exists());
        assert!(manifest_yaml_path.exists());

        let env_contents = fs::read_to_string(&env_file).expect("env file");
        assert!(env_contents.contains("TB_GATEWAY_TLS_CLIENT_CA_OPTIONAL"));
        assert!(!env_contents.contains("# TB_GATEWAY_TLS does not require client auth"));

        let manifest_value: Value = json::from_slice(&fs::read(&manifest_json_path).unwrap())
            .expect("parse optional manifest");
        let manifest_map = manifest_value.as_object().expect("manifest map");
        assert_eq!(
            manifest_map.get("client_auth").and_then(Value::as_str),
            Some("optional")
        );

        fs::remove_dir_all(input).ok();
        fs::remove_dir_all(output).ok();
    }

    #[test]
    fn stage_tls_missing_required_anchor_errors() {
        let _guard = STAGE_LOCK.lock().unwrap();
        let (cert_pem, key_pem, _) = build_pem_identity();
        let input = temp_dir("tls_required_missing_anchor");
        let cert_path = input.join("server.pem");
        let key_path = input.join("server.key");
        fs::write(&cert_path, &cert_pem).expect("write cert");
        fs::write(&key_path, &key_pem).expect("write key");

        convert_tls(
            cert_path.clone(),
            key_path.clone(),
            None,
            input.clone(),
            "identity".to_string(),
            true,
        )
        .expect("convert without anchor");

        let service_dir = input.join("aggregator");
        let spec = format!("aggregator:required={}", service_dir.display());
        let err = stage_tls(
            input.clone(),
            "identity".to_string(),
            vec![spec.parse().unwrap()],
            true,
            None,
        )
        .expect_err("missing anchor should fail");
        assert!(err.contains("requires client auth assets"));

        fs::remove_dir_all(input).ok();
    }

    #[test]
    fn service_target_builds_default_env_prefix() {
        let target: ServiceTarget = "metrics-aggregator=/srv/metrics"
            .parse()
            .expect("parse default");
        assert_eq!(target.env_prefix, "TB_METRICS_AGGREGATOR_TLS");
    }

    #[test]
    fn service_target_respects_env_prefix_override() {
        let target: ServiceTarget = "gateway:optional@TB_GATEWAY_TLS=/srv/gateway"
            .parse()
            .expect("parse override");
        assert_eq!(target.env_prefix, "TB_GATEWAY_TLS");
        assert_eq!(target.mode, ClientAuthMode::Optional);
    }

    fn build_pem_chain() -> (String, [u8; 32]) {
        let (pem, _, verifying) = build_pem_identity();
        let mut chain = String::new();
        chain.push_str(&pem);
        chain.push_str(&pem);
        (chain, verifying)
    }

    fn build_pem_identity() -> (String, String, [u8; 32]) {
        let now = UtcDateTime::now();
        let params = SelfSignedCertParams::builder()
            .subject_cn("test")
            .validity(now - Duration::hours(1), now + Duration::days(1))
            .serial([1; 16])
            .build()
            .unwrap();
        let generated = generate_self_signed_ed25519(&params).expect("generate");
        let cert_pem = encode_pem("CERTIFICATE", &generated.certificate);
        let key_pem = encode_pem("PRIVATE KEY", &generated.private_key);
        (cert_pem, key_pem, generated.public_key)
    }

    fn encode_pem(label: &str, der: &[u8]) -> String {
        let mut out = String::new();
        out.push_str(&format!("-----BEGIN {}-----\n", label));
        let encoded = base64_fp::encode_standard(der);
        for chunk in encoded.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).unwrap());
            out.push('\n');
        }
        out.push_str(&format!("-----END {}-----\n", label));
        out
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let idx = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        dir.push(format!("contract_cli_tls_{prefix}_{timestamp}_{idx}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
