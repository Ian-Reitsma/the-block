use crate::parse_utils::{
    parse_bool, parse_positional_u32, parse_positional_u64, parse_u64, parse_u64_required,
    parse_usize, require_positional, take_string,
};
use base64_fp::encode_standard;
use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::hex;
use foundation_serialization::json::{
    self, Map as JsonMap, Number as JsonNumber, Value as JsonValue,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};
use storage::merkle_proof::MerkleTree;
use storage::{StorageContract, StorageOffer};
use storage_market::{
    AuditReport, ChecksumComparison, ChecksumDigest, ChecksumScope, ImportMode, ImportStats,
    ManifestSource, ManifestStatus, ManifestSummary, StorageImporter,
};
use the_block::{
    generate_keypair, rpc,
    simple_db::EngineKind,
    transaction::{sign_tx, RawTxPayload},
};

pub enum StorageCmd {
    /// Upload data to storage market
    Upload {
        object_id: String,
        provider_id: String,
        bytes: u64,
        shares: u16,
        price: u64,
        retention: u64,
    },
    /// Upload a real file through the drive endpoint
    Put {
        file: String,
        deterministic_fixture: Option<String>,
    },
    /// Challenge a storage provider
    Challenge {
        object_id: String,
        chunk: u64,
        block: u64,
    },
    /// List provider quotas and recent upload metrics
    Providers { json: bool },
    /// Register or update a provider in the DHT catalog
    RegisterProvider {
        provider_id: String,
        region: Option<String>,
        max_capacity_bytes: u64,
        price_per_block: u64,
        escrow_deposit: u64,
        latency_ms: Option<u32>,
        tags: Vec<String>,
    },
    /// Query the DHT marketplace for providers
    DiscoverProviders {
        object_size: u64,
        shares: u16,
        limit: Option<usize>,
        region: Option<String>,
        max_price_per_block: Option<u64>,
        min_success_rate_ppm: Option<u64>,
        json: bool,
    },
    /// Build a storage adoption plan for the wedge narrative
    AdoptionPlan {
        object_size: u64,
        shares: u16,
        retention_blocks: u64,
        region: Option<String>,
        max_price_per_block: Option<u64>,
        min_success_rate_ppm: Option<u64>,
        json: bool,
    },
    /// Toggle maintenance mode for a provider
    Maintenance {
        provider_id: String,
        maintenance: bool,
    },
    /// Show recent repair attempts and outcomes
    RepairHistory { limit: Option<usize>, json: bool },
    /// Trigger the repair loop once and print summary statistics
    RepairRun {},
    /// Force a repair attempt for a manifest chunk
    RepairChunk {
        manifest: String,
        chunk: u32,
        force: bool,
    },
    /// List stored manifests and active coding algorithms
    Manifests { limit: Option<usize>, json: bool },
    /// Inspect and replay legacy storage manifest migrations
    Importer(StorageImporterCmd),
}

fn default_market_dir() -> String {
    env::var("TB_STORAGE_MARKET_DIR").unwrap_or_else(|_| "storage_market".into())
}

fn parse_manifest_source(raw: Option<String>) -> Result<ManifestSource, String> {
    match raw {
        None => Ok(ManifestSource::Auto),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
                Ok(ManifestSource::Auto)
            } else if trimmed.eq_ignore_ascii_case("pending") {
                Ok(ManifestSource::Pending)
            } else if trimmed.eq_ignore_ascii_case("migrated") {
                Ok(ManifestSource::Migrated)
            } else {
                Ok(ManifestSource::File(PathBuf::from(value)))
            }
        }
    }
}

fn parse_checksum_scope(raw: Option<String>) -> Result<ChecksumScope, String> {
    match raw {
        None => Ok(ChecksumScope::ContractsOnly),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("contracts")
                || trimmed.eq_ignore_ascii_case("contract")
                || trimmed.eq_ignore_ascii_case("market/contracts")
            {
                Ok(ChecksumScope::ContractsOnly)
            } else if trimmed.eq_ignore_ascii_case("all")
                || trimmed.eq_ignore_ascii_case("all-cfs")
                || trimmed.eq_ignore_ascii_case("full")
            {
                Ok(ChecksumScope::AllColumnFamilies)
            } else {
                Err(format!("unsupported checksum scope '{trimmed}'"))
            }
        }
    }
}

fn manifest_status_label(status: &ManifestStatus) -> &'static str {
    match status {
        ManifestStatus::Pending { .. } => "pending",
        ManifestStatus::Migrated { .. } => "migrated",
        ManifestStatus::Absent => "absent",
    }
}

fn manifest_status_path(status: &ManifestStatus) -> Option<&Path> {
    match status {
        ManifestStatus::Pending { path } => Some(path.as_path()),
        ManifestStatus::Migrated { path } => Some(path.as_path()),
        ManifestStatus::Absent => None,
    }
}

fn summary_to_json(summary: &ManifestSummary) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "status".into(),
        JsonValue::String(manifest_status_label(&summary.status).into()),
    );
    if let Some(path) = manifest_status_path(&summary.status) {
        map.insert(
            "status_path".into(),
            JsonValue::String(path.to_string_lossy().into_owned()),
        );
    }
    if let Some(path) = summary.source_path.as_ref() {
        map.insert(
            "source_path".into(),
            JsonValue::String(path.to_string_lossy().into_owned()),
        );
    }
    map.insert(
        "total_entries".into(),
        JsonValue::Number(JsonNumber::from(summary.total_entries as u64)),
    );
    map.insert(
        "present".into(),
        JsonValue::Number(JsonNumber::from(summary.present as u64)),
    );
    map.insert(
        "missing".into(),
        JsonValue::Number(JsonNumber::from(summary.missing as u64)),
    );
    map.insert(
        "duplicates".into(),
        JsonValue::Number(JsonNumber::from(summary.duplicates as u64)),
    );
    JsonValue::Object(map)
}

fn stats_to_json(stats: &ImportStats) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "total_entries".into(),
        JsonValue::Number(JsonNumber::from(stats.total_entries as u64)),
    );
    map.insert(
        "applied".into(),
        JsonValue::Number(JsonNumber::from(stats.applied as u64)),
    );
    map.insert(
        "skipped_existing".into(),
        JsonValue::Number(JsonNumber::from(stats.skipped_existing as u64)),
    );
    map.insert(
        "overwritten".into(),
        JsonValue::Number(JsonNumber::from(stats.overwritten as u64)),
    );
    map.insert(
        "no_change".into(),
        JsonValue::Number(JsonNumber::from(stats.no_change as u64)),
    );
    JsonValue::Object(map)
}

fn checksum_scope_label(scope: ChecksumScope) -> &'static str {
    match scope {
        ChecksumScope::ContractsOnly => "contracts",
        ChecksumScope::AllColumnFamilies => "all",
    }
}

fn checksum_to_json(digest: &ChecksumDigest) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "entries".into(),
        JsonValue::Number(JsonNumber::from(digest.entries as u64)),
    );
    map.insert("hash".into(), JsonValue::String(hex::encode(digest.hash)));
    JsonValue::Object(map)
}

fn checksum_comparison_to_json(comparison: &ChecksumComparison) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert(
        "scope".into(),
        JsonValue::String(checksum_scope_label(comparison.scope).into()),
    );
    map.insert("database".into(), checksum_to_json(&comparison.database));
    if let Some(manifest) = comparison.manifest.as_ref() {
        map.insert("manifest".into(), checksum_to_json(manifest));
        map.insert(
            "matches".into(),
            JsonValue::Bool(
                manifest.hash == comparison.database.hash
                    && manifest.entries == comparison.database.entries,
            ),
        );
    } else {
        map.insert("manifest".into(), JsonValue::Null);
    }
    JsonValue::Object(map)
}

fn audit_report_to_json(report: &AuditReport) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("summary".into(), summary_to_json(&report.summary));
    let entries = report
        .entries
        .iter()
        .map(|entry| {
            let mut obj = JsonMap::new();
            obj.insert(
                "key".into(),
                JsonValue::String(hex::encode(entry.key.as_slice())),
            );
            obj.insert("present".into(), JsonValue::Bool(entry.present));
            obj.insert("duplicate".into(), JsonValue::Bool(entry.duplicate));
            JsonValue::Object(obj)
        })
        .collect();
    map.insert("entries".into(), JsonValue::Array(entries));

    let missing = report
        .missing_keys
        .iter()
        .map(|key| JsonValue::String(hex::encode(key)))
        .collect();
    map.insert("missing_keys".into(), JsonValue::Array(missing));

    let duplicates = report
        .duplicate_keys
        .iter()
        .map(|key| JsonValue::String(hex::encode(key)))
        .collect();
    map.insert("duplicate_keys".into(), JsonValue::Array(duplicates));
    JsonValue::Object(map)
}

fn render_manifest_summary(summary: &ManifestSummary, json: bool) {
    if json {
        match json::to_string_pretty(&summary_to_json(summary)) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => eprintln!("format failed: {err}"),
        }
    } else {
        println!(
            "legacy manifest status: {}",
            manifest_status_label(&summary.status)
        );
        if let Some(path) = manifest_status_path(&summary.status) {
            println!("status path: {}", path.display());
        }
        if let Some(path) = summary.source_path.as_ref() {
            println!("resolved path: {}", path.display());
        }
        println!("total entries: {}", summary.total_entries);
        println!("present entries: {}", summary.present);
        println!("missing entries: {}", summary.missing);
        if summary.duplicates > 0 {
            println!("duplicate entries in manifest: {}", summary.duplicates);
        }
    }
}

fn render_import_result(stats: Option<&ImportStats>, summary: &ManifestSummary, json: bool) {
    if json {
        let mut map = JsonMap::new();
        if let Some(stats) = stats {
            map.insert("result".into(), stats_to_json(stats));
        }
        map.insert("summary".into(), summary_to_json(summary));
        match json::to_string_pretty(&JsonValue::Object(map)) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => eprintln!("format failed: {err}"),
        }
    } else {
        if let Some(stats) = stats {
            println!("applied entries: {}", stats.applied);
            println!("overwritten entries: {}", stats.overwritten);
            println!("skipped existing entries: {}", stats.skipped_existing);
            println!("unchanged entries: {}", stats.no_change);
            println!("total manifest entries: {}", stats.total_entries);
            println!();
        }
        render_manifest_summary(summary, false);
    }
}

fn render_audit_report(report: &AuditReport, json: bool) {
    if json {
        match json::to_string_pretty(&audit_report_to_json(report)) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => eprintln!("format failed: {err}"),
        }
        return;
    }

    render_manifest_summary(&report.summary, false);
    if !report.entries.is_empty() {
        println!("sample entries ({} total shown):", report.entries.len());
        for entry in &report.entries {
            println!(
                "  key={} present={} duplicate={}",
                hex::encode(entry.key.as_slice()),
                entry.present,
                entry.duplicate
            );
        }
    }
    if !report.missing_keys.is_empty() {
        println!(
            "sample missing keys: {}",
            report
                .missing_keys
                .iter()
                .map(|key| hex::encode(key))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !report.duplicate_keys.is_empty() {
        println!(
            "sample duplicate keys: {}",
            report
                .duplicate_keys
                .iter()
                .map(|key| hex::encode(key))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn render_verify_result(comparison: &ChecksumComparison, json: bool) {
    if json {
        match json::to_string_pretty(&checksum_comparison_to_json(comparison)) {
            Ok(serialized) => println!("{serialized}"),
            Err(err) => eprintln!("format failed: {err}"),
        }
        return;
    }

    println!("checksum scope: {}", checksum_scope_label(comparison.scope));
    println!("database entries: {}", comparison.database.entries);
    println!("database hash: {}", hex::encode(comparison.database.hash));
    if let Some(manifest) = comparison.manifest.as_ref() {
        println!("manifest entries: {}", manifest.entries);
        println!("manifest hash: {}", hex::encode(manifest.hash));
        let matches = manifest.hash == comparison.database.hash
            && manifest.entries == comparison.database.entries;
        println!("matches: {matches}");
    } else {
        println!("manifest hash: (absent)");
    }
}

fn write_json_to_file(path: &Path, value: &JsonValue) -> Result<(), String> {
    let bytes = json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, bytes).map_err(|err| err.to_string())
}

#[derive(Clone)]
pub enum StorageImporterCmd {
    Audit {
        dir: String,
        source: ManifestSource,
        json: bool,
        out: Option<PathBuf>,
        allow_absent: bool,
    },
    Rerun {
        dir: String,
        source: ManifestSource,
        mode: ImportMode,
        dry_run: bool,
        json: bool,
        allow_absent: bool,
    },
    Verify {
        dir: String,
        source: ManifestSource,
        scope: ChecksumScope,
        json: bool,
        allow_absent: bool,
    },
}

impl StorageImporterCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("storage.importer"),
            "importer",
            "Legacy storage importer utilities",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("storage.importer.audit"),
                "audit",
                "Inspect legacy manifest status",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "dir",
                "dir",
                "Storage market directory to inspect",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "source",
                "source",
                "Manifest source (auto|pending|migrated|<path>)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "out",
                "out",
                "Write JSON audit report to file",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "json",
                "json",
                "Emit JSON instead of human-readable output",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "allow-absent",
                "allow-absent",
                "Exit successfully when no manifest is present",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("storage.importer.rerun"),
                "rerun",
                "Replay legacy manifest entries",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "dir",
                "dir",
                "Storage market directory to operate on",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "source",
                "source",
                "Manifest source (auto|pending|migrated|<path>)",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "overwrite",
                "overwrite",
                "Overwrite existing contract records when replaying",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "dry-run",
                "dry-run",
                "Show summary without applying changes",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "json",
                "json",
                "Emit JSON instead of human-readable output",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "allow-absent",
                "allow-absent",
                "Exit successfully when no manifest is present",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("storage.importer.verify"),
                "verify",
                "Compare manifest checksum against storage",
            )
            .arg(ArgSpec::Option(OptionSpec::new(
                "dir",
                "dir",
                "Storage market directory to inspect",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "source",
                "source",
                "Manifest source (auto|pending|migrated|<path>)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "scope",
                "scope",
                "Checksum scope (contracts|all)",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "json",
                "json",
                "Emit JSON instead of human-readable output",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "allow-absent",
                "allow-absent",
                "Exit successfully when no manifest is present",
            )))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'storage importer'".to_string())?;
        match name {
            "audit" => {
                let dir = take_string(sub_matches, "dir").unwrap_or_else(default_market_dir);
                let source = parse_manifest_source(take_string(sub_matches, "source"))?;
                let json = sub_matches.get_flag("json");
                let out = take_string(sub_matches, "out").map(PathBuf::from);
                let allow_absent = sub_matches.get_flag("allow-absent");
                Ok(Self::Audit {
                    dir,
                    source,
                    json,
                    out,
                    allow_absent,
                })
            }
            "rerun" => {
                let dir = take_string(sub_matches, "dir").unwrap_or_else(default_market_dir);
                let source = parse_manifest_source(take_string(sub_matches, "source"))?;
                let mode = if sub_matches.get_flag("overwrite") {
                    ImportMode::OverwriteExisting
                } else {
                    ImportMode::InsertMissing
                };
                let dry_run = sub_matches.get_flag("dry-run");
                let json = sub_matches.get_flag("json");
                let allow_absent = sub_matches.get_flag("allow-absent");
                Ok(Self::Rerun {
                    dir,
                    source,
                    mode,
                    dry_run,
                    json,
                    allow_absent,
                })
            }
            "verify" => {
                let dir = take_string(sub_matches, "dir").unwrap_or_else(default_market_dir);
                let source = parse_manifest_source(take_string(sub_matches, "source"))?;
                let scope = parse_checksum_scope(take_string(sub_matches, "scope"))?;
                let json = sub_matches.get_flag("json");
                let allow_absent = sub_matches.get_flag("allow-absent");
                Ok(Self::Verify {
                    dir,
                    source,
                    scope,
                    json,
                    allow_absent,
                })
            }
            other => Err(format!(
                "unknown subcommand '{other}' for 'storage importer'"
            )),
        }
    }
}

impl StorageCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("storage"), "storage", "Storage market utilities")
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.upload"),
                    "upload",
                    "Upload data to storage market",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "object_id",
                    "Storage object identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "provider_id",
                    "Storage provider identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "bytes",
                    "Total bytes to store",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "shares",
                    "Number of shares",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "price",
                    "Price per block in BLOCK",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "retention",
                    "Retention duration in blocks",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.put"),
                    "put",
                    "Upload a file through the drive cache",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "file",
                    "File path to upload",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "deterministic-fixture",
                    "deterministic-fixture",
                    "Use deterministic fixture payload instead of the supplied file",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.challenge"),
                    "challenge",
                    "Challenge a storage provider",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "object_id",
                    "Storage object identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "chunk",
                    "Chunk index",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "block",
                    "Block height",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.providers"),
                    "providers",
                    "List provider quotas and recent upload metrics",
                )
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.register_provider"),
                    "register-provider",
                    "Register or refresh a provider profile in the storage marketplace",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "provider_id",
                    "Storage provider identifier",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "region",
                    "region",
                    "Optional region hint",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "max-capacity-bytes",
                        "max-capacity-bytes",
                        "Maximum chunk capacity advertised by the provider",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "price-per-block",
                        "price-per-block",
                        "Price per BLOCK to charge per block",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "escrow-deposit",
                        "escrow-deposit",
                        "Escrow deposit (in BLOCK) to cover retention guarantees",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "latency-ms",
                    "latency-ms",
                    "Optional latency in milliseconds",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("tag", "tag", "Attach an arbitrary provider tag")
                        .multiple(true),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.discover_providers"),
                    "discover-providers",
                    "Run the storage DHT marketplace discovery flow",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "object-size",
                        "object-size",
                        "Size of the object to store (bytes)",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("shares", "shares", "Number of shares (minimum 1)")
                        .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "limit",
                    "limit",
                    "Maximum providers to return",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "region",
                    "region",
                    "Optional region filter",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "max-price-per-block",
                    "max-price-per-block",
                    "Upper bound for price per block",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "min-success-rate-ppm",
                    "min-success-rate-ppm",
                    "Minimum success rate (in ppm) required",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.adoption_plan"),
                    "adoption-plan",
                    "Summarize the storage adoption plan for the storage wedge",
                )
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "object-size",
                        "object-size",
                        "Size of the object to store (bytes)",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new("shares", "shares", "Number of shares (minimum 1)")
                        .required(true),
                ))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "retention-blocks",
                        "retention-blocks",
                        "Desired retention in blocks",
                    )
                    .required(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "region",
                    "region",
                    "Optional region hint for discovery",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "max-price-per-block",
                    "max-price-per-block",
                    "Upper bound on price per block",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "min-success-rate-ppm",
                    "min-success-rate-ppm",
                    "Minimum provider success rate (ppm)",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.maintenance"),
                    "maintenance",
                    "Toggle maintenance mode for a provider",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "provider_id",
                    "Storage provider identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "maintenance",
                        "maintenance",
                        "Set maintenance mode (true/false)",
                    )
                    .default("true"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.repair_history"),
                    "repair-history",
                    "Show recent repair attempts and outcomes",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "limit",
                    "limit",
                    "Maximum entries to display",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.repair_run"),
                    "repair-run",
                    "Trigger the repair loop once and print summary statistics",
                )
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.repair_chunk"),
                    "repair-chunk",
                    "Force a repair attempt for a manifest chunk",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "manifest",
                    "Manifest identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "chunk",
                    "Chunk index",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "force",
                    "force",
                    "Force the repair attempt even if not due",
                )))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(
                    CommandId("storage.manifests"),
                    "manifests",
                    "List stored manifests and active coding algorithms",
                )
                .arg(ArgSpec::Option(OptionSpec::new(
                    "limit",
                    "limit",
                    "Maximum manifests to display",
                )))
                .arg(ArgSpec::Flag(FlagSpec::new(
                    "json",
                    "json",
                    "Emit JSON instead of human-readable output",
                )))
                .build(),
            )
            .subcommand(StorageImporterCmd::command())
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'storage'".to_string())?;

        match name {
            "upload" => {
                let object_id = require_positional(sub_matches, "object_id")?;
                let provider_id = require_positional(sub_matches, "provider_id")?;
                let bytes = parse_positional_u64(sub_matches, "bytes")?;
                let shares_raw = require_positional(sub_matches, "shares")?;
                let shares = shares_raw
                    .parse::<u16>()
                    .map_err(|_| format!("invalid value '{shares_raw}' for 'shares'"))?;
                let price = parse_positional_u64(sub_matches, "price")?;
                let retention = parse_positional_u64(sub_matches, "retention")?;
                Ok(StorageCmd::Upload {
                    object_id,
                    provider_id,
                    bytes,
                    shares,
                    price,
                    retention,
                })
            }
            "put" => {
                let file = require_positional(sub_matches, "file")?;
                let deterministic_fixture = take_string(sub_matches, "deterministic-fixture");
                Ok(StorageCmd::Put {
                    file,
                    deterministic_fixture,
                })
            }
            "challenge" => {
                let object_id = require_positional(sub_matches, "object_id")?;
                let chunk = parse_positional_u64(sub_matches, "chunk")?;
                let block = parse_positional_u64(sub_matches, "block")?;
                Ok(StorageCmd::Challenge {
                    object_id,
                    chunk,
                    block,
                })
            }
            "providers" => Ok(StorageCmd::Providers {
                json: sub_matches.get_flag("json"),
            }),
            "register-provider" => {
                let provider_id = require_positional(sub_matches, "provider_id")?;
                let region = take_string(sub_matches, "region");
                let max_capacity = parse_u64_required(
                    take_string(sub_matches, "max-capacity-bytes"),
                    "max-capacity-bytes",
                )?;
                let price_per_block = parse_u64_required(
                    take_string(sub_matches, "price-per-block"),
                    "price-per-block",
                )?;
                let escrow_deposit = parse_u64_required(
                    take_string(sub_matches, "escrow-deposit"),
                    "escrow-deposit",
                )?;
                let latency = parse_u64(take_string(sub_matches, "latency-ms"), "latency-ms")?
                    .map(|value| value.min(u32::MAX as u64) as u32);
                let tags = sub_matches.get_strings("tag");
                Ok(StorageCmd::RegisterProvider {
                    provider_id,
                    region,
                    max_capacity_bytes: max_capacity,
                    price_per_block,
                    escrow_deposit,
                    latency_ms: latency,
                    tags,
                })
            }
            "discover-providers" => {
                let object_size =
                    parse_u64_required(take_string(sub_matches, "object-size"), "object-size")?;
                let shares_raw = parse_u64_required(take_string(sub_matches, "shares"), "shares")?;
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?;
                let region = take_string(sub_matches, "region");
                let max_price_per_block = parse_u64(
                    take_string(sub_matches, "max-price-per-block"),
                    "max-price-per-block",
                )?;
                let min_success_rate_ppm = parse_u64(
                    take_string(sub_matches, "min-success-rate-ppm"),
                    "min-success-rate-ppm",
                )?;
                let json = sub_matches.get_flag("json");
                Ok(StorageCmd::DiscoverProviders {
                    object_size,
                    shares: shares_raw.min(u16::MAX as u64) as u16,
                    limit,
                    region,
                    max_price_per_block,
                    min_success_rate_ppm,
                    json,
                })
            }
            "adoption-plan" => {
                let object_size =
                    parse_u64_required(take_string(sub_matches, "object-size"), "object-size")?;
                let shares_raw = parse_u64_required(take_string(sub_matches, "shares"), "shares")?;
                let retention_blocks = parse_u64_required(
                    take_string(sub_matches, "retention-blocks"),
                    "retention-blocks",
                )?;
                let region = take_string(sub_matches, "region");
                let max_price_per_block = parse_u64(
                    take_string(sub_matches, "max-price-per-block"),
                    "max-price-per-block",
                )?;
                let min_success_rate_ppm = parse_u64(
                    take_string(sub_matches, "min-success-rate-ppm"),
                    "min-success-rate-ppm",
                )?;
                let json = sub_matches.get_flag("json");
                Ok(StorageCmd::AdoptionPlan {
                    object_size,
                    shares: shares_raw.min(u16::MAX as u64) as u16,
                    retention_blocks,
                    region,
                    max_price_per_block,
                    min_success_rate_ppm,
                    json,
                })
            }
            "maintenance" => {
                let provider_id = require_positional(sub_matches, "provider_id")?;
                let maintenance =
                    parse_bool(take_string(sub_matches, "maintenance"), true, "maintenance")?;
                Ok(StorageCmd::Maintenance {
                    provider_id,
                    maintenance,
                })
            }
            "repair-history" => {
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?;
                let json = sub_matches.get_flag("json");
                Ok(StorageCmd::RepairHistory { limit, json })
            }
            "repair-run" => Ok(StorageCmd::RepairRun {}),
            "repair-chunk" => {
                let manifest = require_positional(sub_matches, "manifest")?;
                let chunk = parse_positional_u32(sub_matches, "chunk")?;
                let force = sub_matches.get_flag("force");
                Ok(StorageCmd::RepairChunk {
                    manifest,
                    chunk,
                    force,
                })
            }
            "manifests" => {
                let limit = parse_usize(take_string(sub_matches, "limit"), "limit")?;
                let json = sub_matches.get_flag("json");
                Ok(StorageCmd::Manifests { limit, json })
            }
            "importer" => {
                let cmd = StorageImporterCmd::from_matches(sub_matches)?;
                Ok(StorageCmd::Importer(cmd))
            }
            other => Err(format!("unknown subcommand '{other}' for 'storage'")),
        }
    }
}

fn handle_importer(cmd: StorageImporterCmd) {
    match cmd {
        StorageImporterCmd::Audit {
            dir,
            source,
            json,
            out,
            allow_absent,
        } => match StorageImporter::open(&dir) {
            Ok(importer) => match importer.audit(source) {
                Ok(report) => {
                    if report.summary.source_path.is_none() && !allow_absent {
                        eprintln!(
                            "no legacy manifest found (status: {})",
                            manifest_status_label(&report.summary.status)
                        );
                        process::exit(1);
                    }

                    if let Some(path) = out {
                        let value = audit_report_to_json(&report);
                        if let Err(err) = write_json_to_file(&path, &value) {
                            eprintln!("failed to write audit report {}: {err}", path.display());
                            process::exit(1);
                        }
                    }

                    render_audit_report(&report, json);
                }
                Err(err) => {
                    eprintln!("storage importer audit failed: {err}");
                    process::exit(1);
                }
            },
            Err(err) => {
                eprintln!("failed to open storage market at {dir}: {err}");
                process::exit(1);
            }
        },
        StorageImporterCmd::Rerun {
            dir,
            source,
            mode,
            dry_run,
            json,
            allow_absent,
        } => match StorageImporter::open(&dir) {
            Ok(importer) => {
                let summary_before = match importer.summarize(source.clone()) {
                    Ok(summary) => summary,
                    Err(err) => {
                        eprintln!("storage importer inspection failed: {err}");
                        process::exit(1);
                    }
                };
                if summary_before.source_path.is_none() && !allow_absent {
                    eprintln!(
                        "no legacy manifest found (status: {})",
                        manifest_status_label(&summary_before.status)
                    );
                    process::exit(1);
                }
                if dry_run {
                    render_import_result(None, &summary_before, json);
                    return;
                }
                let stats = match importer.import(source.clone(), mode) {
                    Ok(stats) => stats,
                    Err(err) => {
                        eprintln!("storage importer replay failed: {err}");
                        process::exit(1);
                    }
                };
                let summary_after = match importer.summarize(source) {
                    Ok(summary) => summary,
                    Err(err) => {
                        eprintln!("storage importer post-run summary failed: {err}");
                        process::exit(1);
                    }
                };
                render_import_result(Some(&stats), &summary_after, json);
            }
            Err(err) => {
                eprintln!("failed to open storage market at {dir}: {err}");
                process::exit(1);
            }
        },
        StorageImporterCmd::Verify {
            dir,
            source,
            scope,
            json,
            allow_absent,
        } => match StorageImporter::open(&dir) {
            Ok(importer) => match importer.verify(source, scope) {
                Ok(comparison) => {
                    if comparison.manifest.is_none() && !allow_absent {
                        eprintln!("no legacy manifest available for verification");
                        process::exit(1);
                    }
                    render_verify_result(&comparison, json);
                    if let Some(manifest) = comparison.manifest.as_ref() {
                        let matches = manifest.hash == comparison.database.hash
                            && manifest.entries == comparison.database.entries;
                        if !matches {
                            process::exit(2);
                        }
                    }
                }
                Err(err) => {
                    eprintln!("storage importer verification failed: {err}");
                    process::exit(1);
                }
            },
            Err(err) => {
                eprintln!("failed to open storage market at {dir}: {err}");
                process::exit(1);
            }
        },
    }
}

fn deterministic_chunks(object_id: &str, chunk_count: usize) -> Vec<Vec<u8>> {
    let count = chunk_count.max(1);
    (0..count)
        .map(|i| {
            let mut hasher = crypto_suite::hashing::blake3::Hasher::new();
            hasher.update(object_id.as_bytes());
            hasher.update(&i.to_le_bytes());
            hasher.finalize().as_bytes().to_vec()
        })
        .collect()
}

fn build_merkle_tree(chunks: &[Vec<u8>]) -> MerkleTree {
    let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
    MerkleTree::build(&chunk_refs).expect("build Merkle tree")
}

pub fn handle(cmd: StorageCmd) {
    match cmd {
        StorageCmd::Upload {
            object_id,
            provider_id,
            bytes,
            shares,
            price,
            retention,
        } => {
            let chunk_count = shares.max(1) as usize;
            let chunks = deterministic_chunks(&object_id, chunk_count);
            let tree = build_merkle_tree(&chunks);
            let contract = StorageContract {
                object_id: object_id.clone(),
                provider_id: provider_id.clone(),
                original_bytes: bytes,
                shares,
                price_per_block: price,
                start_block: 0,
                retention_blocks: retention,
                next_payment_block: 1,
                accrued: 0,
                total_deposit: 0,
                last_payment_block: None,
                storage_root: tree.root,
            };
            let total = price * retention;
            let payload = RawTxPayload {
                from_: "wallet".into(),
                to: provider_id.clone(),
                amount_consumer: total,
                amount_industrial: 0,
                fee: 0,
                pct: 100,
                nonce: 0,
                memo: Vec::new(),
            };
            let (sk, _pk) = generate_keypair();
            let _signed = sign_tx(&sk, &payload).expect("signing");
            let offer = StorageOffer::new(provider_id, bytes, price, retention);
            let resp = rpc::storage::upload(contract, vec![offer]);
            println!("{}", resp);
            println!("reserved {} BLOCK", total);
        }
        StorageCmd::Put {
            file,
            deterministic_fixture,
        } => {
            let data = if let Some(fixture) = deterministic_fixture.as_deref() {
                deterministic_chunks(fixture, 4)
                    .into_iter()
                    .flatten()
                    .collect()
            } else {
                match fs::read(&file) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        eprintln!("failed to read {}: {err}", file);
                        process::exit(1);
                    }
                }
            };
            let payload = encode_standard(&data);
            let resp = rpc::storage::drive_put(&payload);
            let object_id = resp
                .get("object_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if object_id.is_empty() {
                eprintln!("drive upload failed: {resp}");
                process::exit(1);
            }
            let size = resp
                .get("size")
                .and_then(|value| value.as_u64())
                .unwrap_or(data.len() as u64);
            println!("drive object id: {object_id}");
            println!("size: {size} bytes");
            if let Some(link) = resp.get("share_url").and_then(|value| value.as_str()) {
                println!("share link: {link}");
            }
            if let Some(fixture) = deterministic_fixture {
                println!("deterministic fixture: {fixture}");
            }
        }
        StorageCmd::Challenge {
            object_id,
            chunk,
            block,
        } => {
            let chunks = deterministic_chunks(&object_id, 4);
            let tree = build_merkle_tree(&chunks);
            let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();
            let tree_idx = (chunk as usize) % chunk_refs.len();
            let proof = tree
                .generate_proof(tree_idx as u64, &chunk_refs)
                .expect("generate proof for demo chunks");
            let resp =
                rpc::storage::challenge(&object_id, None, chunk, &chunks[tree_idx], &proof, block);
            println!("{}", resp);
        }
        StorageCmd::Providers { json } => {
            let resp = rpc::storage::provider_profiles();
            if json {
                println!("{}", resp);
            } else if let Some(list) = resp.get("profiles").and_then(|v| v.as_array()) {
                if let Some(engine) = resp.get("engine").and_then(|v| v.as_object()) {
                    let pipeline = engine
                        .get("pipeline")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let rent = engine
                        .get("rent_escrow")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    println!("storage pipeline engine: {pipeline} (rent escrow: {rent})");
                    let recommended = EngineKind::default_for_build().label();
                    if pipeline != recommended || rent != recommended {
                        println!(
                            "warning: recommended storage engine is {recommended}; consider migrating via tools/storage_migrate"
                        );
                    }
                    if engine
                        .get("legacy_mode")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        println!(
                            "warning: storage legacy mode is enabled and will be removed in the next release"
                        );
                    }
                    println!();
                }
                println!(
                    "{:>20} {:>12} {:>8} {:>10} {:>8} {:>8} {:>6}",
                    "provider", "quota_bytes", "chunk", "throughput", "loss", "rtt_ms", "maint"
                );
                for entry in list {
                    let provider = entry
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let quota = entry
                        .get("quota_bytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let chunk = entry
                        .get("preferred_chunk")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let throughput = entry
                        .get("throughput_bps")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let loss = entry.get("loss").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let rtt = entry.get("rtt_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let maintenance = entry
                        .get("maintenance")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    println!(
                        "{:>20} {:>12} {:>8} {:>10.0} {:>8.3} {:>8.1} {:>6}",
                        provider,
                        quota,
                        chunk,
                        throughput,
                        loss,
                        rtt,
                        if maintenance { "yes" } else { "no" }
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::RegisterProvider {
            provider_id,
            region,
            max_capacity_bytes,
            price_per_block,
            escrow_deposit,
            latency_ms,
            tags,
        } => {
            let resp = rpc::storage::register_provider(
                &provider_id,
                region.as_deref(),
                max_capacity_bytes,
                price_per_block,
                escrow_deposit,
                latency_ms,
                tags,
            );
            println!("{}", resp);
        }
        StorageCmd::DiscoverProviders {
            object_size,
            shares,
            limit,
            region,
            max_price_per_block,
            min_success_rate_ppm,
            json,
        } => {
            let resp = rpc::storage::discover_providers(
                object_size,
                shares,
                limit,
                region.as_deref(),
                max_price_per_block,
                min_success_rate_ppm,
            );
            if json {
                println!("{}", resp);
            } else if let Some(list) = resp.get("providers").and_then(|v| v.as_array()) {
                println!(
                    "{:>20} {:>10} {:>10} {:>12} {:>10} {:>8} {:<}",
                    "provider", "region", "price", "capacity", "success%", "latency", "tags"
                );
                for entry in list {
                    let provider = entry
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let region = entry.get("region").and_then(|v| v.as_str()).unwrap_or("-");
                    let price = entry
                        .get("price_per_block")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let capacity = entry
                        .get("capacity_bytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let success_ppm = entry
                        .get("success_rate_ppm")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let success_pct = success_ppm as f64 / 10000.0;
                    let latency = entry
                        .get("latency_ms")
                        .and_then(|v| v.as_u64())
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".into());
                    let tags_str = entry
                        .get("tags")
                        .and_then(|v| v.as_array())
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "{:>20} {:>10} {:>10} {:>12} {:>10.2} {:>8} {:<}",
                        provider, region, price, capacity, success_pct, latency, tags_str
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::AdoptionPlan {
            object_size,
            shares,
            retention_blocks,
            region,
            max_price_per_block,
            min_success_rate_ppm,
            json,
        } => {
            let resp = rpc::storage::adoption_plan(
                object_size,
                shares,
                retention_blocks,
                region.as_deref(),
                max_price_per_block,
                min_success_rate_ppm,
            );
            if json {
                println!("{}", resp);
            } else {
                render_adoption_plan(&resp);
            }
        }
        StorageCmd::Maintenance {
            provider_id,
            maintenance,
        } => {
            let resp = rpc::storage::set_provider_maintenance(&provider_id, maintenance);
            println!("{}", resp);
        }
        StorageCmd::RepairHistory { limit, json } => {
            let resp = rpc::storage::repair_history(limit);
            if json {
                println!("{}", resp);
            } else if let Some(entries) = resp.get("entries").and_then(|v| v.as_array()) {
                println!(
                    "{:<40} {:>8} {:>10} {:>12} {:<}",
                    "manifest", "chunk", "bytes", "status", "error"
                );
                for entry in entries {
                    let manifest = entry
                        .get("manifest")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let chunk = entry
                        .get("chunk")
                        .and_then(|v| v.as_u64())
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "-".into());
                    let bytes = entry.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                    let status = entry.get("status").and_then(|v| v.as_str()).unwrap_or("-");
                    let error = entry.get("error").and_then(|v| v.as_str()).unwrap_or("");
                    println!(
                        "{:<40} {:>8} {:>10} {:>12} {:<}",
                        manifest, chunk, bytes, status, error
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::RepairRun {} => {
            let resp = rpc::storage::repair_run();
            println!("{}", resp);
        }
        StorageCmd::RepairChunk {
            manifest,
            chunk,
            force,
        } => {
            let resp = rpc::storage::repair_chunk(&manifest, chunk, force);
            println!("{}", resp);
        }
        StorageCmd::Manifests { limit, json } => {
            let resp = rpc::storage::manifest_summaries(limit);
            if json {
                println!("{}", resp);
            } else if let Some(entries) = resp.get("manifests").and_then(|v| v.as_array()) {
                if let Some(policy) = resp.get("policy").and_then(|v| v.as_object()) {
                    if let Some(erasure) = policy.get("erasure").and_then(|v| v.as_object()) {
                        let algorithm = erasure
                            .get("algorithm")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let fallback = erasure
                            .get("fallback")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let emergency = erasure
                            .get("emergency")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        println!(
                            "erasure policy: {algorithm} (fallback={}, emergency={})",
                            fallback, emergency
                        );
                    }
                    if let Some(compression) = policy.get("compression").and_then(|v| v.as_object())
                    {
                        let algorithm = compression
                            .get("algorithm")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let fallback = compression
                            .get("fallback")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let emergency = compression
                            .get("emergency")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        println!(
                            "compression policy: {algorithm} (fallback={}, emergency={})",
                            fallback, emergency
                        );
                    }
                    println!();
                }
                println!(
                    "{:<64} {:<16} {:<16} {:<6} {:<6}",
                    "manifest", "erasure", "compression", "e_fb", "c_fb"
                );
                for entry in entries {
                    let manifest = entry
                        .get("manifest")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let erasure = entry.get("erasure").and_then(|v| v.as_str()).unwrap_or("-");
                    let compression = entry
                        .get("compression")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let compression_level = entry.get("compression_level").and_then(|v| v.as_i64());
                    let erasure_fb = entry
                        .get("erasure_fallback")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let compression_fb = entry
                        .get("compression_fallback")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let mut erasure_display = erasure.to_string();
                    if erasure_fb {
                        erasure_display.push('*');
                    }
                    let mut compression_display = if let Some(level) = compression_level {
                        format!("{compression}({level})")
                    } else {
                        compression.to_string()
                    };
                    if compression_fb {
                        compression_display.push('*');
                    }
                    println!(
                        "{:<64} {:<16} {:<16} {:<6} {:<6}",
                        manifest,
                        erasure_display,
                        compression_display,
                        if erasure_fb { "yes" } else { "no" },
                        if compression_fb { "yes" } else { "no" }
                    );
                }
            } else {
                println!("{}", resp);
            }
        }
        StorageCmd::Importer(cmd) => handle_importer(cmd),
    }
}

fn render_adoption_plan(resp: &JsonValue) {
    let plan_name = resp
        .get("plan_name")
        .and_then(|value| value.as_str())
        .unwrap_or("storage_adoption_wedge");
    let object_size = resp
        .get("object_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let shares = resp
        .get("shares")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let retention_blocks = resp
        .get("retention_blocks")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let coverage = plan_value_to_string(resp.get("coverage_percentage"));
    let selected_providers = resp
        .get("selected_provider_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let required_providers = resp
        .get("required_provider_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let total_cost = plan_value_to_string(resp.get("estimated_total_cost"));
    let primary_provider = resp
        .get("primary_provider")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let filters = resp
        .get("search_filters")
        .and_then(|value| value.as_object());
    let region_filter = filters
        .and_then(|map| map.get("region"))
        .map(|value| plan_value_to_string(Some(value)))
        .unwrap_or_else(|| "-".into());
    let max_price = filters
        .and_then(|map| map.get("max_price_per_block"))
        .map(|value| plan_value_to_string(Some(value)))
        .unwrap_or_else(|| "-".into());
    let min_success = filters
        .and_then(|map| map.get("min_success_rate_ppm"))
        .map(|value| plan_value_to_string(Some(value)))
        .unwrap_or_else(|| "-".into());

    println!("Plan: {plan_name}");
    println!(
        "  object size: {object_size} bytes · shares: {shares} · retention: {retention_blocks} blocks"
    );
    println!(
        "  providers: {selected_providers}/{required_providers} · coverage: {coverage}% · total cost: {total_cost} BLOCK"
    );
    println!("  primary provider: {primary_provider}");
    println!(
        "  filters: region={region_filter} · max price={max_price} · min success={min_success}"
    );

    if let Some(providers) = resp.get("providers").and_then(|value| value.as_array()) {
        if !providers.is_empty() {
            println!();
            println!(
                "{:>20} {:>10} {:>8} {:>10} {:>8} {:>14} {:>14} {:<}",
                "provider",
                "region",
                "price",
                "success%",
                "latency",
                "cost/share",
                "cost/shares",
                "tags"
            );
            for entry in providers {
                let provider = entry
                    .get("provider")
                    .and_then(|value| value.as_str())
                    .unwrap_or("-");
                let region = entry
                    .get("region")
                    .and_then(|value| value.as_str())
                    .unwrap_or("-");
                let price = entry
                    .get("price_per_block")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                let success = plan_value_to_string(entry.get("success_rate_pct"));
                let latency = plan_value_to_string(entry.get("latency_ms"));
                let cost_share = plan_value_to_string(entry.get("estimated_cost_per_share"));
                let cost_shares = plan_value_to_string(entry.get("estimated_cost_for_shares"));
                let tags = entry
                    .get("tags")
                    .and_then(|value| value.as_array())
                    .map(|tags| {
                        tags.iter()
                            .filter_map(|value| value.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "-".into());
                println!(
                    "{:>20} {:>10} {:>8} {:>10} {:>8} {:>14} {:>14} {:<}",
                    provider, region, price, success, latency, cost_share, cost_shares, tags
                );
            }
        }
    }

    if let Some(actions) = resp
        .get("recommended_actions")
        .and_then(|value| value.as_array())
    {
        if !actions.is_empty() {
            println!();
            println!("Recommended actions:");
            for (idx, action) in actions.iter().enumerate() {
                let title = plan_value_to_string(action.get("title"));
                let detail = plan_value_to_string(action.get("detail"));
                println!("  {}. {} – {}", idx + 1, title, detail);
            }
        }
    }

    if let Some(signals) = resp
        .get("monitoring_signals")
        .and_then(|value| value.as_array())
    {
        if !signals.is_empty() {
            println!();
            println!("Monitoring signals:");
            for signal in signals {
                let metric = plan_value_to_string(signal.get("metric"));
                let goal = plan_value_to_string(signal.get("goal"));
                let reason = plan_value_to_string(signal.get("reason"));
                println!("  - {metric} (goal {goal}): {reason}");
            }
        }
    }

    if let Some(failures) = resp
        .get("failure_workflow")
        .and_then(|value| value.as_array())
    {
        if !failures.is_empty() {
            println!();
            println!("Failure workflow:");
            for entry in failures {
                let trigger = plan_value_to_string(entry.get("trigger"));
                let response = plan_value_to_string(entry.get("response"));
                println!("  • {trigger} -> {response}");
            }
        }
    }
}

fn plan_value_to_string(value: Option<&JsonValue>) -> String {
    match value {
        Some(JsonValue::String(value)) => value.clone(),
        Some(JsonValue::Number(number)) => number.to_string(),
        Some(JsonValue::Bool(flag)) => flag.to_string(),
        Some(other) => other.to_string(),
        None => "-".into(),
    }
}
