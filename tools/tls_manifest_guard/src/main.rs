use foundation_serialization::{json, Deserialize, Serialize};
use foundation_time::UtcDateTime;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() {
    if let Err(exit) = run() {
        eprintln!("{}", exit.message);
        std::process::exit(exit.code);
    }
}

#[derive(Debug)]
struct ExitError {
    code: i32,
    message: String,
}

impl ExitError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

struct Config {
    manifests: Vec<PathBuf>,
    env_file: Option<PathBuf>,
    allow_stale_reminder: bool,
    report_path: Option<PathBuf>,
}

fn run() -> Result<(), ExitError> {
    let config = parse_args()?;
    let env_map = match &config.env_file {
        Some(path) => Some(load_env_file(path).map_err(|err| ExitError::new(1, err))?),
        None => None,
    };

    let mut had_errors = false;
    let mut outcomes = Vec::new();
    for manifest in &config.manifests {
        match validate_manifest(manifest, env_map.as_ref(), config.allow_stale_reminder) {
            Ok(warnings) => {
                for warning in &warnings {
                    eprintln!("warning: {}", warning);
                }
                outcomes.push(ManifestOutcome::passed(manifest, warnings));
            }
            Err(errors) => {
                had_errors = true;
                for error in &errors {
                    eprintln!("error: {}", error);
                }
                outcomes.push(ManifestOutcome::failed(manifest, errors));
            }
        }
    }

    if let Some(report_path) = &config.report_path {
        write_report(report_path, &outcomes)?;
    }

    if had_errors {
        Err(ExitError::new(2, "manifest validation failed"))
    } else {
        Ok(())
    }
}

fn parse_args() -> Result<Config, ExitError> {
    let mut manifests = Vec::new();
    let mut env_file = None;
    let mut allow_stale_reminder = false;
    let mut report_path = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                let Some(path) = args.next() else {
                    return Err(ExitError::new(64, "--manifest requires a path"));
                };
                manifests.push(PathBuf::from(path));
            }
            "--env-file" => {
                let Some(path) = args.next() else {
                    return Err(ExitError::new(64, "--env-file requires a path"));
                };
                env_file = Some(PathBuf::from(path));
            }
            "--allow-stale-reminder" => {
                allow_stale_reminder = true;
            }
            "--report" => {
                let Some(path) = args.next() else {
                    return Err(ExitError::new(64, "--report requires a path"));
                };
                report_path = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                print_usage();
                return Err(ExitError::new(0, String::new()));
            }
            other => {
                return Err(ExitError::new(
                    64,
                    format!("unrecognised argument '{other}'"),
                ));
            }
        }
    }

    if manifests.is_empty() {
        return Err(ExitError::new(
            64,
            "at least one --manifest path is required",
        ));
    }

    Ok(Config {
        manifests,
        env_file,
        allow_stale_reminder,
        report_path,
    })
}

fn print_usage() {
    println!("tls-manifest-guard --manifest <path> [--manifest <path> ...] [--env-file <path>] [--allow-stale-reminder] [--report <path>]");
}

#[derive(Serialize, Clone)]
#[serde(crate = "foundation_serialization::serde", rename_all = "snake_case")]
enum ManifestStatus {
    Passed,
    Failed,
}

#[derive(Serialize, Clone)]
#[serde(crate = "foundation_serialization::serde")]
struct ManifestOutcome {
    manifest: String,
    status: ManifestStatus,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl ManifestOutcome {
    fn passed(path: &Path, warnings: Vec<String>) -> Self {
        Self {
            manifest: display(path),
            status: ManifestStatus::Passed,
            warnings,
            errors: Vec::new(),
        }
    }

    fn failed(path: &Path, errors: Vec<String>) -> Self {
        Self {
            manifest: display(path),
            status: ManifestStatus::Failed,
            warnings: Vec::new(),
            errors,
        }
    }
}

fn write_report(path: &Path, outcomes: &[ManifestOutcome]) -> Result<(), ExitError> {
    let file = fs::File::create(path).map_err(|err| {
        ExitError::new(
            1,
            format!("failed to open report '{}': {}", display(path), err),
        )
    })?;
    let mut writer = io::BufWriter::new(file);
    let payload = json::to_vec(&outcomes.to_vec()).map_err(|err| {
        ExitError::new(
            1,
            format!("failed to serialize report '{}': {}", display(path), err),
        )
    })?;
    writer.write_all(&payload).map_err(|err| {
        ExitError::new(
            1,
            format!("failed to write report '{}': {}", display(path), err),
        )
    })?;
    writer.flush().map_err(|err| {
        ExitError::new(
            1,
            format!("failed to flush report '{}': {}", display(path), err),
        )
    })
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct ServiceManifest {
    version: u8,
    generated_at: Option<String>,
    service: String,
    directory: String,
    env_prefix: String,
    client_auth: String,
    staged_files: Vec<String>,
    env_exports: Vec<EnvExport>,
    #[serde(default)]
    renewal_timestamp: Option<String>,
    #[serde(default)]
    renewal_reminder: Option<String>,
    #[serde(default)]
    renewal_window_days: Option<u32>,
}

#[derive(Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
struct EnvExport {
    key: String,
    value: String,
}

fn validate_manifest(
    path: &Path,
    env_map: Option<&HashMap<String, String>>,
    allow_stale_reminder: bool,
) -> Result<Vec<String>, Vec<String>> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let data = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(vec![format!(
                "{}: unable to read manifest: {}",
                display(path),
                err
            )]);
        }
    };

    let manifest: ServiceManifest = match json::from_slice(&data) {
        Ok(manifest) => manifest,
        Err(err) => {
            return Err(vec![format!(
                "{}: unable to parse manifest: {}",
                display(path),
                err
            )]);
        }
    };

    if manifest.version != 1 {
        errors.push(format!(
            "{}: unsupported manifest version {}; expected 1",
            display(path),
            manifest.version
        ));
    }

    if manifest.service.trim().is_empty() {
        errors.push(format!("{}: service name is empty", display(path)));
    }

    let directory = PathBuf::from(&manifest.directory);
    if !directory.exists() {
        errors.push(format!(
            "{}: declared directory '{}' does not exist",
            display(path),
            manifest.directory
        ));
    }
    let canonical_directory = if directory.exists() {
        match directory.canonicalize() {
            Ok(dir) => Some(dir),
            Err(err) => {
                warnings.push(format!(
                    "{}: failed to resolve canonical directory '{}': {}",
                    display(path),
                    manifest.directory,
                    err
                ));
                None
            }
        }
    } else {
        None
    };

    if let Some(raw) = manifest.generated_at.as_deref() {
        if let Err(err) = parse_timestamp(raw) {
            warnings.push(format!(
                "{}: generated_at '{}' could not be parsed: {}",
                display(path),
                raw,
                err
            ));
        }
    }

    if manifest.env_prefix.trim().is_empty() {
        errors.push(format!("{}: env_prefix is empty", display(path)));
    } else if manifest
        .env_prefix
        .chars()
        .any(|ch| ch.is_ascii_lowercase())
    {
        warnings.push(format!(
            "{}: env_prefix '{}' contains lowercase characters",
            display(path),
            manifest.env_prefix
        ));
    }

    match manifest.client_auth.as_str() {
        "required" | "optional" | "none" => {}
        other => errors.push(format!(
            "{}: client_auth value '{}' is not recognised",
            display(path),
            other
        )),
    }

    let mut staged_seen = HashSet::new();
    for staged in &manifest.staged_files {
        if !staged_seen.insert(staged.as_str()) {
            warnings.push(format!(
                "{}: staged file '{}' appears multiple times",
                display(path),
                staged
            ));
        }
        let staged_path = PathBuf::from(staged);
        if !staged_path.exists() {
            errors.push(format!(
                "{}: staged file '{}' is missing",
                display(path),
                staged
            ));
        }
        if let (Some(dir), Ok(staged_canon)) =
            (canonical_directory.as_ref(), staged_path.canonicalize())
        {
            if !staged_canon.starts_with(dir) {
                errors.push(format!(
                    "{}: staged file '{}' is outside declared directory '{}'",
                    display(path),
                    staged,
                    manifest.directory
                ));
            }
        }
    }

    let staged_set: HashSet<&str> = manifest
        .staged_files
        .iter()
        .map(|entry| entry.as_str())
        .collect();

    let expected_prefix = normalized_env_prefix(&manifest.env_prefix);
    let mut export_seen = HashSet::new();
    for export in &manifest.env_exports {
        if !export_seen.insert(export.key.as_str()) {
            warnings.push(format!(
                "{}: env export '{}' appears multiple times",
                display(path),
                export.key
            ));
        }
        if export.key.trim().is_empty() {
            errors.push(format!("{}: env export with empty key", display(path)));
            continue;
        }
        if export.value.trim().is_empty() {
            errors.push(format!(
                "{}: env export '{}' has an empty value",
                display(path),
                export.key
            ));
            continue;
        }

        if !export.key.starts_with(&expected_prefix) {
            errors.push(format!(
                "{}: env export '{}' does not match env_prefix '{}'",
                display(path),
                export.key,
                manifest.env_prefix
            ));
        }

        let export_path = PathBuf::from(&export.value);
        if !export_path.exists() {
            errors.push(format!(
                "{}: env export '{}' points to missing file '{}'",
                display(path),
                export.key,
                export.value
            ));
        }
        if let (Some(dir), Ok(export_canon)) =
            (canonical_directory.as_ref(), export_path.canonicalize())
        {
            if !export_canon.starts_with(dir) {
                errors.push(format!(
                    "{}: env export '{}' points outside declared directory '{}'",
                    display(path),
                    export.key,
                    manifest.directory
                ));
            }
        }
        if !staged_set.contains(export.value.as_str()) {
            warnings.push(format!(
                "{}: env export '{}' references '{}' which is not listed in staged_files",
                display(path),
                export.key,
                export.value
            ));
        }
        if let Some(env_map) = env_map {
            match env_map.get(&export.key) {
                Some(value) if value == &export.value => {}
                Some(value) => {
                    errors.push(format!(
                        "{}: env file maps '{}' to '{}' but manifest expects '{}'",
                        display(path),
                        export.key,
                        value,
                        export.value
                    ));
                }
                None => {
                    errors.push(format!(
                        "{}: env file missing export for '{}'",
                        display(path),
                        export.key
                    ));
                }
            }
        }
    }

    if let Some(env_map) = env_map {
        let manifest_keys: HashSet<&str> = manifest
            .env_exports
            .iter()
            .map(|export| export.key.as_str())
            .collect();
        for key in env_map.keys() {
            if key.starts_with(&expected_prefix) && !manifest_keys.contains(key.as_str()) {
                warnings.push(format!(
                    "{}: env file defines '{}' which is not declared in manifest",
                    display(path),
                    key
                ));
            }
        }
    }

    if let Some(raw) = manifest.renewal_timestamp.as_deref() {
        match parse_timestamp(raw) {
            Ok(expiry) => {
                if expiry <= UtcDateTime::now() {
                    errors.push(format!(
                        "{}: certificate renewal timestamp {} has passed",
                        display(path),
                        raw
                    ));
                }
            }
            Err(err) => {
                errors.push(format!(
                    "{}: invalid renewal_timestamp '{}': {}",
                    display(path),
                    raw,
                    err
                ));
            }
        }
    }

    if let Some(reminder_str) = manifest.renewal_reminder.as_deref() {
        match parse_timestamp(reminder_str) {
            Ok(reminder) => {
                if !allow_stale_reminder && reminder <= UtcDateTime::now() {
                    errors.push(format!(
                        "{}: renewal_reminder {} has elapsed",
                        display(path),
                        reminder_str
                    ));
                }
            }
            Err(err) => {
                errors.push(format!(
                    "{}: invalid renewal_reminder '{}': {}",
                    display(path),
                    reminder_str,
                    err
                ));
            }
        }
    }

    if let Some(window) = manifest.renewal_window_days {
        if window == 0 {
            warnings.push(format!("{}: renewal_window_days is zero", display(path)));
        }
    }

    if errors.is_empty() {
        Ok(warnings)
    } else {
        Err(errors)
    }
}

fn load_env_file(path: &Path) -> Result<HashMap<String, String>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("{}: unable to read env file: {}", display(path), err))?;
    let mut map = HashMap::new();
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(format!(
                "{}:{}: expected KEY=VALUE entry",
                display(path),
                index + 1
            ));
        };
        let cleaned = strip_optional_quotes(value.trim());
        map.insert(key.trim().to_string(), cleaned);
    }
    Ok(map)
}

fn strip_optional_quotes(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let last = value.len() - 1;
        if (bytes[0] == b'"' && bytes[last] == b'"') || (bytes[0] == b'\'' && bytes[last] == b'\'')
        {
            return value[1..last].to_string();
        }
    }
    value.to_string()
}

fn parse_timestamp(input: &str) -> Result<UtcDateTime, String> {
    let bytes = input.as_bytes();
    if bytes.len() != 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return Err("timestamp must be ISO-8601 (YYYY-MM-DDTHH:MM:SSZ)".into());
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'Z' {
        return Err("timestamp must be ISO-8601 (YYYY-MM-DDTHH:MM:SSZ)".into());
    }
    let year = parse_decimal(&bytes[0..4]).map_err(|err| err.to_string())? as i32;
    let month = parse_decimal(&bytes[5..7]).map_err(|err| err.to_string())? as u8;
    let day = parse_decimal(&bytes[8..10]).map_err(|err| err.to_string())? as u8;
    let hour = parse_decimal(&bytes[11..13]).map_err(|err| err.to_string())? as u8;
    let minute = parse_decimal(&bytes[14..16]).map_err(|err| err.to_string())? as u8;
    let second = parse_decimal(&bytes[17..19]).map_err(|err| err.to_string())? as u8;
    timestamp_from_components(year, month, day, hour, minute, second).map_err(|err| err.to_string())
}

fn parse_decimal(bytes: &[u8]) -> Result<u32, &'static str> {
    if bytes.is_empty() {
        return Err("expected digits");
    }
    let mut value = 0u32;
    for &b in bytes {
        if !(b'0'..=b'9').contains(&b) {
            return Err("expected digits");
        }
        value = value * 10 + (b - b'0') as u32;
    }
    Ok(value)
}

fn timestamp_from_components(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> Result<UtcDateTime, &'static str> {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("invalid timestamp components");
    }
    let days = days_from_civil(year, month, day);
    let mut seconds_total = days as i128 * 86_400;
    seconds_total += hour as i128 * 3_600;
    seconds_total += minute as i128 * 60;
    seconds_total += second as i128;
    if seconds_total < i64::MIN as i128 || seconds_total > i64::MAX as i128 {
        return Err("timestamp out of range");
    }
    UtcDateTime::from_unix_timestamp(seconds_total as i64).map_err(|_| "timestamp out of range")
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
    era * 146_097 + doe - 719_468
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn normalized_env_prefix(prefix: &str) -> String {
    if prefix.ends_with('_') {
        prefix.to_string()
    } else {
        format!("{prefix}_")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!(
            "tls_manifest_guard_{suffix}_{}_{}",
            std::process::id(),
            timestamp
        ));
        path
    }

    fn escape_path(path: &Path) -> String {
        path.to_string_lossy().replace("\\", "\\\\")
    }

    #[test]
    fn report_serialization_writes_expected_json() {
        let path = unique_path("report");
        let outcomes = vec![
            ManifestOutcome::passed(
                Path::new("/etc/the-block/tls/node/tls-manifest.json"),
                vec!["staging file uses legacy suffix".to_string()],
            ),
            ManifestOutcome::failed(
                Path::new("/etc/the-block/tls/gateway/tls-manifest.json"),
                vec!["renewal reminder is stale".to_string()],
            ),
        ];
        write_report(&path, &outcomes).expect("report written");
        let raw = std::fs::read_to_string(&path).expect("report readable");
        let value: json::Value = json::from_str(&raw).expect("report json");
        let array = value.as_array().expect("array outcomes");
        assert_eq!(array.len(), 2);
        let first = &array[0];
        assert_eq!(
            first.get("status").and_then(json::Value::as_str),
            Some("passed")
        );
        let second = &array[1];
        assert_eq!(
            second
                .get("errors")
                .and_then(json::Value::as_array)
                .map(|v| v.len()),
            Some(1)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_env_file_rejects_malformed_line() {
        let path = unique_path("env");
        fs::write(&path, "export TLS_CERT_PATH\n").unwrap();
        let error = load_env_file(&path).expect_err("malformed env line should error");
        assert!(error.contains("expected KEY=VALUE entry"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_env_file_strips_wrapping_quotes() {
        let path = unique_path("env_quotes");
        fs::write(
            &path,
            "TB_NODE_CERT=\"/etc/node/cert.pem\"\nTB_NODE_KEY='/etc/node/key.pem'\n",
        )
        .unwrap();
        let map = load_env_file(&path).expect("quoted env parsed");
        assert_eq!(
            map.get("TB_NODE_CERT").map(String::as_str),
            Some("/etc/node/cert.pem")
        );
        assert_eq!(
            map.get("TB_NODE_KEY").map(String::as_str),
            Some("/etc/node/key.pem")
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn validate_manifest_respects_allow_stale_reminder_flag() {
        let base_dir = unique_path("manifest");
        fs::create_dir_all(&base_dir).unwrap();
        let service_dir = base_dir.join("service");
        fs::create_dir_all(&service_dir).unwrap();
        let staged_file = service_dir.join("tls.cert");
        fs::write(&staged_file, "dummy").unwrap();

        let manifest_path = base_dir.join("manifest.json");
        let staged_value = staged_file.to_string_lossy().to_string();
        let manifest = format!(
            r#"{{
                "version": 1,
                "service": "node",
                "directory": "{directory}",
                "env_prefix": "TB_NODE",
                "client_auth": "required",
                "staged_files": ["{staged}"],
                "env_exports": [{{"key": "TB_NODE_CERT", "value": "{staged}"}}],
                "renewal_timestamp": "2099-01-01T00:00:00Z",
                "renewal_reminder": "2000-01-01T00:00:00Z"
            }}"#,
            directory = escape_path(&service_dir),
            staged = escape_path(&staged_file)
        );
        fs::write(&manifest_path, manifest).unwrap();

        let mut env_map = HashMap::new();
        env_map.insert("TB_NODE_CERT".into(), staged_value.clone());

        let errors = validate_manifest(&manifest_path, Some(&env_map), false)
            .expect_err("stale reminder should be rejected");
        assert!(errors
            .iter()
            .any(|msg| msg.contains("renewal_reminder") && msg.contains("has elapsed")));

        let warnings = validate_manifest(&manifest_path, Some(&env_map), true)
            .expect("stale reminder allowed");
        assert!(warnings
            .iter()
            .all(|msg| !msg.contains("renewal_reminder") || !msg.contains("has elapsed")));

        let _ = fs::remove_file(&manifest_path);
        let _ = fs::remove_file(&staged_file);
        let _ = fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn validate_manifest_rejects_env_keys_without_prefix() {
        let base_dir = unique_path("manifest_prefix");
        fs::create_dir_all(&base_dir).unwrap();
        let service_dir = base_dir.join("service");
        fs::create_dir_all(&service_dir).unwrap();
        let staged_file = service_dir.join("tls.key");
        fs::write(&staged_file, "dummy").unwrap();

        let manifest_path = base_dir.join("manifest.json");
        let manifest = format!(
            r#"{{
                "version": 1,
                "service": "node",
                "directory": "{directory}",
                "env_prefix": "TB_NODE",
                "client_auth": "required",
                "staged_files": ["{staged}"],
                "env_exports": [{{"key": "NODE_CERT", "value": "{staged}"}}],
                "renewal_timestamp": "2099-01-01T00:00:00Z"
            }}"#,
            directory = escape_path(&service_dir),
            staged = escape_path(&staged_file)
        );
        fs::write(&manifest_path, manifest).unwrap();

        let result = validate_manifest(&manifest_path, None, false)
            .expect_err("env key without prefix should error");
        assert!(result
            .iter()
            .any(|msg| msg.contains("does not match env_prefix")));

        let _ = fs::remove_file(&manifest_path);
        let _ = fs::remove_file(&staged_file);
        let _ = fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn strip_optional_quotes_preserves_unbalanced_inputs() {
        assert_eq!(
            strip_optional_quotes("\"/etc/node/cert.pem"),
            "\"/etc/node/cert.pem"
        );
        assert_eq!(
            strip_optional_quotes("/etc/node/key.pem'"),
            "/etc/node/key.pem'"
        );
    }

    #[test]
    fn validate_manifest_warns_on_env_file_extras() {
        let base_dir = unique_path("manifest_env_extra");
        fs::create_dir_all(&base_dir).unwrap();
        let service_dir = base_dir.join("service");
        fs::create_dir_all(&service_dir).unwrap();
        let staged_file = service_dir.join("tls.cert");
        fs::write(&staged_file, "dummy").unwrap();

        let manifest_path = base_dir.join("manifest.json");
        let staged_value = escape_path(&staged_file);
        let manifest = format!(
            r#"{{
                "version": 1,
                "service": "node",
                "directory": "{directory}",
                "env_prefix": "TB_NODE",
                "client_auth": "required",
                "staged_files": ["{staged}"],
                "env_exports": [{{"key": "TB_NODE_CERT", "value": "{staged}"}}]
            }}"#,
            directory = escape_path(&service_dir),
            staged = staged_value
        );
        fs::write(&manifest_path, manifest).unwrap();

        let mut env_map = HashMap::new();
        env_map.insert("TB_NODE_CERT".into(), staged_value.clone());
        env_map.insert("TB_NODE_UNUSED".into(), "/tmp/unused".into());

        let warnings = validate_manifest(&manifest_path, Some(&env_map), false)
            .expect("validation should succeed with warnings");
        assert!(warnings
            .iter()
            .any(|msg| msg.contains("env file defines 'TB_NODE_UNUSED'")));

        let _ = fs::remove_file(&manifest_path);
        let _ = fs::remove_file(&staged_file);
        let _ = fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn validate_manifest_rejects_paths_outside_directory() {
        let base_dir = unique_path("manifest_paths");
        fs::create_dir_all(&base_dir).unwrap();
        let service_dir = base_dir.join("service");
        fs::create_dir_all(&service_dir).unwrap();
        let staged_dir = base_dir.join("staged");
        fs::create_dir_all(&staged_dir).unwrap();
        let staged_file = staged_dir.join("tls.cert");
        fs::write(&staged_file, "dummy").unwrap();

        let manifest_path = base_dir.join("manifest.json");
        let staged_value = escape_path(&staged_file);
        let manifest = format!(
            r#"{{
                "version": 1,
                "service": "node",
                "directory": "{directory}",
                "env_prefix": "TB_NODE",
                "client_auth": "required",
                "staged_files": ["{staged}"],
                "env_exports": [{{"key": "TB_NODE_CERT", "value": "{staged}"}}]
            }}"#,
            directory = escape_path(&service_dir),
            staged = staged_value
        );
        fs::write(&manifest_path, manifest).unwrap();

        let mut env_map = HashMap::new();
        env_map.insert("TB_NODE_CERT".into(), staged_value.clone());

        let errors = validate_manifest(&manifest_path, Some(&env_map), false)
            .expect_err("staged file outside directory should error");
        assert!(errors
            .iter()
            .any(|msg| msg.contains("outside declared directory")));

        let _ = fs::remove_file(&manifest_path);
        let _ = fs::remove_file(&staged_file);
        let _ = fs::remove_dir_all(&base_dir);
    }
}
