use foundation_serialization::json::{self, Map, Value};
use std::{collections::HashMap, fs, path::PathBuf};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub deprecated: bool,
}

#[derive(Debug)]
pub enum DashboardError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: foundation_serialization::Error,
    },
    InvalidStructure(&'static str),
    InvalidMetric {
        name: Option<String>,
        field: &'static str,
    },
}

impl DashboardError {
    fn invalid_metric<N: Into<Option<String>>>(name: N, field: &'static str) -> Self {
        DashboardError::InvalidMetric {
            name: name.into(),
            field,
        }
    }
}

impl std::fmt::Display for DashboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DashboardError::Io { path, source } => {
                write!(f, "failed to read '{}': {}", path.display(), source)
            }
            DashboardError::Parse { path, source } => {
                write!(f, "failed to parse '{}': {}", path.display(), source)
            }
            DashboardError::InvalidStructure(msg) => f.write_str(msg),
            DashboardError::InvalidMetric { name, field } => match name {
                Some(name) => write!(
                    f,
                    "invalid metric '{}': missing/invalid '{}' field",
                    name, field
                ),
                None => write!(f, "invalid metric entry: missing/invalid '{}' field", field),
            },
        }
    }
}

impl std::error::Error for DashboardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DashboardError::Io { source, .. } => Some(source),
            DashboardError::Parse { source, .. } => Some(source),
            DashboardError::InvalidStructure(_) | DashboardError::InvalidMetric { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricValue {
    Float(f64),
    Integer(i64),
    Unsigned(u64),
}

impl MetricValue {}

pub type MetricSnapshot = HashMap<String, MetricValue>;

const TLS_PANEL_TITLE: &str = "TLS env warnings (5m delta)";
const TLS_PANEL_EXPR: &str = "sum by (prefix, code)(increase(tls_env_warning_total[5m]))";
const TLS_LAST_SEEN_PANEL_TITLE: &str = "TLS env warnings (last seen)";
const TLS_LAST_SEEN_EXPR: &str = "max by (prefix, code)(tls_env_warning_last_seen_seconds)";
const TLS_FRESHNESS_PANEL_TITLE: &str = "TLS env warnings (age seconds)";
const TLS_FRESHNESS_EXPR: &str =
    "clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)";
const TLS_ACTIVE_PANEL_TITLE: &str = "TLS env warnings (active snapshots)";
const TLS_ACTIVE_EXPR: &str = "tls_env_warning_active_snapshots";
const TLS_STALE_PANEL_TITLE: &str = "TLS env warnings (stale snapshots)";
const TLS_STALE_EXPR: &str = "tls_env_warning_stale_snapshots";
const TLS_RETENTION_PANEL_TITLE: &str = "TLS env warnings (retention seconds)";
const TLS_RETENTION_EXPR: &str = "tls_env_warning_retention_seconds";
const TLS_DETAIL_FINGERPRINT_PANEL_TITLE: &str = "TLS env warnings (detail fingerprint hash)";
const TLS_DETAIL_FINGERPRINT_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_detail_fingerprint)";
const TLS_VARIABLES_FINGERPRINT_PANEL_TITLE: &str = "TLS env warnings (variables fingerprint hash)";
const TLS_VARIABLES_FINGERPRINT_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_variables_fingerprint)";
const TLS_DETAIL_UNIQUE_PANEL_TITLE: &str = "TLS env warnings (unique detail fingerprints)";
const TLS_DETAIL_UNIQUE_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_detail_unique_fingerprints)";
const TLS_VARIABLES_UNIQUE_PANEL_TITLE: &str = "TLS env warnings (unique variables fingerprints)";
const TLS_VARIABLES_UNIQUE_EXPR: &str =
    "max by (prefix, code)(tls_env_warning_variables_unique_fingerprints)";
const TLS_DETAIL_FINGERPRINT_DELTA_PANEL_TITLE: &str =
    "TLS env warnings (detail fingerprint 5m delta)";
const TLS_DETAIL_FINGERPRINT_DELTA_EXPR: &str =
    "sum by (prefix, code, fingerprint)(increase(tls_env_warning_detail_fingerprint_total[5m]))";
const TLS_VARIABLES_FINGERPRINT_DELTA_PANEL_TITLE: &str =
    "TLS env warnings (variables fingerprint 5m delta)";
const TLS_VARIABLES_FINGERPRINT_DELTA_EXPR: &str =
    "sum by (prefix, code, fingerprint)(increase(tls_env_warning_variables_fingerprint_total[5m]))";
const DEP_STATUS_PANEL_TITLE: &str = "Dependency registry check status";
const DEP_STATUS_EXPR: &str = "dependency_registry_check_status";
const DEP_COUNTS_PANEL_TITLE: &str = "Dependency registry drift counts";
const DEP_COUNTS_EXPR: &str = "dependency_registry_check_counts";
const DEP_VIOLATION_TOTAL_PANEL_TITLE: &str = "Dependency policy violations (total)";
const DEP_VIOLATION_TOTAL_EXPR: &str = "dependency_policy_violation_total";
const DEP_VIOLATION_PANEL_TITLE: &str = "Dependency policy violations by crate";
const DEP_VIOLATION_EXPR: &str = "dependency_policy_violation";
const AD_ROW_TITLE: &str = "Advertising";
const AD_ATTESTATION_PANEL_TITLE: &str = "Selection attestations (5m delta)";
const AD_ATTESTATION_EXPR: &str =
    "sum by (kind, result)(increase(ad_selection_attestation_total[5m]))";
const AD_ATTESTATION_LEGEND: &str = "{{kind}} · {{result}}";
const AD_PROOF_LATENCY_PANEL_TITLE: &str = "Selection proof verification latency p95";
const AD_PROOF_LATENCY_EXPR: &str =
    "histogram_quantile(0.95, sum by (le, circuit)(rate(ad_selection_proof_verify_seconds_bucket[5m])))";
const AD_PROOF_LATENCY_LEGEND: &str = "{{circuit}}";
const AD_BUDGET_PROGRESS_PANEL_TITLE: &str = "Budget progress by campaign";
const AD_BUDGET_PROGRESS_EXPR: &str = "ad_budget_progress";
const AD_BUDGET_PROGRESS_LEGEND: &str = "{{campaign}}";
const AD_BUDGET_SHADOW_PANEL_TITLE: &str = "Budget shadow price spikes (5m delta)";
const AD_BUDGET_SHADOW_EXPR: &str =
    "sum by (campaign)(increase(ad_budget_shadow_price_spike_total[5m]))";
const AD_BUDGET_SHADOW_LEGEND: &str = "{{campaign}}";
const AD_DUAL_PRICE_PANEL_TITLE: &str = "Budget dual price";
const AD_DUAL_PRICE_EXPR: &str = "ad_budget_dual_price";
const AD_DUAL_PRICE_LEGEND: &str = "{{campaign}}";
const TREASURY_COUNT_PANEL_TITLE: &str = "Treasury disbursements (count by status)";
const TREASURY_COUNT_EXPR: &str = "treasury_disbursement_count";
const TREASURY_AMOUNT_PANEL_TITLE: &str = "Treasury disbursement CT by status";
const TREASURY_AMOUNT_EXPR: &str = "treasury_disbursement_amount";
const TREASURY_SNAPSHOT_AGE_PANEL_TITLE: &str = "Treasury snapshot age (seconds)";
const TREASURY_SNAPSHOT_AGE_EXPR: &str = "treasury_disbursement_snapshot_age_seconds";
const TREASURY_SCHEDULED_OLDEST_PANEL_TITLE: &str =
    "Oldest scheduled treasury disbursement age (seconds)";
const TREASURY_SCHEDULED_OLDEST_EXPR: &str = "treasury_disbursement_scheduled_oldest_age_seconds";
const TREASURY_NEXT_EPOCH_PANEL_TITLE: &str = "Next treasury disbursement epoch";
const TREASURY_NEXT_EPOCH_EXPR: &str = "treasury_disbursement_next_epoch";
const TREASURY_LEASE_RELEASED_PANEL_TITLE: &str = "treasury_executor_lease_released";
const TREASURY_LEASE_RELEASED_EXPR: &str = "treasury_executor_lease_released";
const RANGE_BOOST_ROW_TITLE: &str = "Range Boost";
const RANGE_BOOST_FORWARDER_FAIL_PANEL_TITLE: &str = "RangeBoost forwarder failures (5m delta)";
const RANGE_BOOST_FORWARDER_FAIL_EXPR: &str = "increase(range_boost_forwarder_fail_total[5m])";
const RANGE_BOOST_FORWARDER_FAIL_LEGEND: &str = "{{__name__}}";
const RANGE_BOOST_ENQUEUE_ERROR_PANEL_TITLE: &str = "RangeBoost enqueue errors (5m delta)";
const RANGE_BOOST_ENQUEUE_ERROR_EXPR: &str = "increase(range_boost_enqueue_error_total[5m])";
const RANGE_BOOST_ENQUEUE_ERROR_LEGEND: &str = "{{__name__}}";
const RANGE_BOOST_TOGGLE_LATENCY_PANEL_TITLE: &str = "RangeBoost toggle latency p95";
const RANGE_BOOST_TOGGLE_LATENCY_EXPR: &str =
    "histogram_quantile(0.95, sum(rate(range_boost_toggle_latency_seconds_bucket[5m])) by (le))";
const RANGE_BOOST_TOGGLE_LATENCY_LEGEND: &str = "p95";
const RANGE_BOOST_QUEUE_DEPTH_PANEL_TITLE: &str = "RangeBoost queue depth";
const RANGE_BOOST_QUEUE_DEPTH_EXPR: &str = "range_boost_queue_depth";
const RANGE_BOOST_QUEUE_OLDEST_PANEL_TITLE: &str = "RangeBoost queue oldest age (seconds)";
const RANGE_BOOST_QUEUE_OLDEST_EXPR: &str = "range_boost_queue_oldest_seconds";
const PAYOUT_ROW_TITLE: &str = "Block Payouts";
const PAYOUT_READ_PANEL_TITLE: &str = "Read subsidy payouts (5m delta)";
const PAYOUT_READ_EXPR: &str = "sum by (role)(increase(explorer_block_payout_read_total[5m]))";
const PAYOUT_READ_LAST_SEEN_PANEL_TITLE: &str = "Read subsidy last seen (timestamp)";
const PAYOUT_READ_LAST_SEEN_EXPR: &str =
    "max by (role)(explorer_block_payout_read_last_seen_timestamp)";
const PAYOUT_READ_STALENESS_PANEL_TITLE: &str = "Read subsidy staleness (seconds)";
const PAYOUT_READ_STALENESS_EXPR: &str =
    "clamp_min(time() - max by (role)(explorer_block_payout_read_last_seen_timestamp), 0)";
const PAYOUT_AD_PANEL_TITLE: &str = "Advertising payouts (5m delta)";
const PAYOUT_AD_EXPR: &str = "sum by (role)(increase(explorer_block_payout_ad_total[5m]))";
const PAYOUT_AD_LAST_SEEN_PANEL_TITLE: &str = "Advertising payout last seen (timestamp)";
const PAYOUT_AD_LAST_SEEN_EXPR: &str =
    "max by (role)(explorer_block_payout_ad_last_seen_timestamp)";
const PAYOUT_AD_STALENESS_PANEL_TITLE: &str = "Advertising payout staleness (seconds)";
const PAYOUT_AD_STALENESS_EXPR: &str =
    "clamp_min(time() - max by (role)(explorer_block_payout_ad_last_seen_timestamp), 0)";
const PAYOUT_ROLE_LEGEND: &str = "{{role}}";
const SLA_OUTCOME_PANEL_TITLE: &str = "Explorer compute SLA outcomes";
const SLA_OUTCOME_EXPR: &str = "explorer_compute_sla_outcome_total";
const SLA_OUTCOME_LEGEND: &str = "{{outcome}}";
const SLA_FRESHNESS_PANEL_TITLE: &str = "Explorer compute SLA staleness (seconds)";
const SLA_FRESHNESS_EXPR: &str = "clamp_min(time() - explorer_compute_sla_last_seen_timestamp, 0)";
const SLA_POLL_ERROR_PANEL_TITLE: &str = "Explorer compute SLA poll errors";
const SLA_POLL_ERROR_EXPR: &str = "explorer_compute_sla_poll_error_total";
const BRIDGE_REWARD_CLAIMS_PANEL_TITLE: &str = "bridge_reward_claims_total (5m delta)";
const BRIDGE_REWARD_CLAIMS_EXPR: &str = "increase(bridge_reward_claims_total[5m])";
const BRIDGE_REWARD_APPROVALS_PANEL_TITLE: &str =
    "bridge_reward_approvals_consumed_total (5m delta)";
const BRIDGE_REWARD_APPROVALS_EXPR: &str = "increase(bridge_reward_approvals_consumed_total[5m])";
const BRIDGE_SETTLEMENT_RESULTS_PANEL_TITLE: &str = "bridge_settlement_results_total (5m delta)";
const BRIDGE_SETTLEMENT_RESULTS_EXPR: &str =
    "sum by (result, reason)(increase(bridge_settlement_results_total[5m]))";
const BRIDGE_SETTLEMENT_RESULTS_LEGEND: &str = "{{result}} · {{reason}}";
const BRIDGE_DISPUTE_OUTCOMES_PANEL_TITLE: &str = "bridge_dispute_outcomes_total (5m delta)";
const BRIDGE_DISPUTE_OUTCOMES_EXPR: &str =
    "sum by (kind, outcome)(increase(bridge_dispute_outcomes_total[5m]))";
const BRIDGE_DISPUTE_OUTCOMES_LEGEND: &str = "{{kind}} · {{outcome}}";
const BRIDGE_LIQUIDITY_LOCKED_PANEL_TITLE: &str = "bridge_liquidity_locked_total (5m delta)";
const BRIDGE_LIQUIDITY_LOCKED_EXPR: &str =
    "sum by (asset)(increase(bridge_liquidity_locked_total[5m]))";
const BRIDGE_LIQUIDITY_UNLOCKED_PANEL_TITLE: &str = "bridge_liquidity_unlocked_total (5m delta)";
const BRIDGE_LIQUIDITY_UNLOCKED_EXPR: &str =
    "sum by (asset)(increase(bridge_liquidity_unlocked_total[5m]))";
const BRIDGE_LIQUIDITY_MINTED_PANEL_TITLE: &str = "bridge_liquidity_minted_total (5m delta)";
const BRIDGE_LIQUIDITY_MINTED_EXPR: &str =
    "sum by (asset)(increase(bridge_liquidity_minted_total[5m]))";
const BRIDGE_LIQUIDITY_BURNED_PANEL_TITLE: &str = "bridge_liquidity_burned_total (5m delta)";
const BRIDGE_LIQUIDITY_BURNED_EXPR: &str =
    "sum by (asset)(increase(bridge_liquidity_burned_total[5m]))";
const BRIDGE_LIQUIDITY_LEGEND: &str = "{{asset}}";
const BRIDGE_REMEDIATION_PANEL_TITLE: &str = "bridge_remediation_action_total (5m delta)";
const BRIDGE_REMEDIATION_EXPR: &str =
    "sum by (action, playbook)(increase(bridge_remediation_action_total[5m]))";
const BRIDGE_REMEDIATION_LEGEND: &str = "{{action}} · {{playbook}}";
const BRIDGE_REMEDIATION_DISPATCH_PANEL_TITLE: &str =
    "bridge_remediation_dispatch_total (5m delta)";
const BRIDGE_REMEDIATION_DISPATCH_EXPR: &str =
    "sum by (action, playbook, target, status)(increase(bridge_remediation_dispatch_total[5m]))";
const BRIDGE_REMEDIATION_DISPATCH_LEGEND: &str = "{{action}} · {{target}} · {{status}}";
const BRIDGE_REMEDIATION_ACK_PANEL_TITLE: &str = "bridge_remediation_dispatch_ack_total (5m delta)";
const BRIDGE_REMEDIATION_ACK_EXPR: &str =
    "sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))";
const BRIDGE_REMEDIATION_ACK_LEGEND: &str = "{{action}} · {{target}} · {{state}}";
const BRIDGE_REMEDIATION_ACK_LATENCY_PANEL_TITLE: &str =
    "bridge_remediation_ack_latency_seconds (p50/p95)";
const BRIDGE_REMEDIATION_ACK_LATENCY_METRIC: &str = "bridge_remediation_ack_latency_seconds";
const BRIDGE_REMEDIATION_SPOOL_PANEL_TITLE: &str = "bridge_remediation_spool_artifacts";
const BRIDGE_REMEDIATION_SPOOL_EXPR: &str = "bridge_remediation_spool_artifacts";
const BRIDGE_ANOMALY_PANEL_TITLE: &str = "bridge_anomaly_total (5m delta)";
const BRIDGE_ANOMALY_EXPR: &str = "increase(bridge_anomaly_total[5m])";
const BRIDGE_METRIC_DELTA_PANEL_TITLE: &str = "bridge_metric_delta";
const BRIDGE_METRIC_DELTA_EXPR: &str = "sum by (metric)(bridge_metric_delta)";
const BRIDGE_METRIC_DELTA_DESCRIPTION: &str = "Per-scrape bridge counter deltas grouped by metric";
const CHAOS_ROW_TITLE: &str = "Chaos Readiness";
const CHAOS_READINESS_PANEL_TITLE: &str = "Chaos readiness";
const CHAOS_READINESS_EXPR: &str = "chaos_readiness";
const CHAOS_READINESS_LEGEND: &str = "{{module}} · {{scenario}}";
const CHAOS_SITE_READINESS_PANEL_TITLE: &str = "Chaos site readiness";
const CHAOS_SITE_READINESS_EXPR: &str = "chaos_site_readiness";
const CHAOS_SITE_READINESS_LEGEND: &str = "{{module}} · {{scenario}} · {{site}} · {{provider}}";
const CHAOS_BREACH_PANEL_TITLE: &str = "Chaos SLA breaches (5m delta)";
const CHAOS_BREACH_DELTA_EXPR: &str =
    "sum by (module, scenario)(increase(chaos_sla_breach_total[5m]))";
const CHAOS_BREACH_LEGEND: &str = "{{module}} · {{scenario}}";
const BRIDGE_METRIC_RATE_PANEL_TITLE: &str = "bridge_metric_rate_per_second";
const BRIDGE_METRIC_RATE_EXPR: &str = "sum by (metric)(bridge_metric_rate_per_second)";
const BRIDGE_METRIC_RATE_DESCRIPTION: &str = "Per-second bridge counter growth grouped by metric";

impl Metric {
    fn from_value(value: &Value) -> Result<Self, DashboardError> {
        let map = match value {
            Value::Object(map) => map,
            _ => {
                return Err(DashboardError::InvalidStructure(
                    "metric entries must be JSON objects",
                ))
            }
        };

        let name = Self::string_field(map, "name")
            .ok_or_else(|| DashboardError::invalid_metric(None::<String>, "name"))?;
        let description = Self::string_field(map, "description").unwrap_or_default();
        let unit = Self::string_field(map, "unit").unwrap_or_default();
        let deprecated = match map.get("deprecated") {
            Some(Value::Bool(flag)) => *flag,
            Some(_) => {
                return Err(DashboardError::invalid_metric(
                    Some(name.clone()),
                    "deprecated",
                ))
            }
            None => false,
        };

        Ok(Self {
            name,
            description,
            unit,
            deprecated,
        })
    }

    fn string_field(map: &Map, key: &str) -> Option<String> {
        map.get(key).and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
    }
}

pub fn generate_dashboard(
    metrics_path: &str,
    overrides_path: Option<&str>,
) -> Result<Value, DashboardError> {
    let metrics = load_metrics_spec(metrics_path)?;
    let overrides = match overrides_path {
        Some(path) => Some(read_json(path)?),
        None => None,
    };
    generate(&metrics, overrides)
}

pub fn load_metrics_spec(path: &str) -> Result<Vec<Metric>, DashboardError> {
    let value = read_json(path)?;
    extract_metrics(&value)
}

fn read_json(path: &str) -> Result<Value, DashboardError> {
    let path_buf = PathBuf::from(path);
    let raw = fs::read_to_string(&path_buf).map_err(|source| DashboardError::Io {
        path: path_buf.clone(),
        source,
    })?;
    json::value_from_str(&raw).map_err(|source| DashboardError::Parse {
        path: path_buf,
        source,
    })
}

fn extract_metrics(root: &Value) -> Result<Vec<Metric>, DashboardError> {
    let metrics_value = match root {
        Value::Object(map) => map.get("metrics"),
        _ => None,
    }
    .ok_or(DashboardError::InvalidStructure(
        "metrics specification must be an object containing a 'metrics' array",
    ))?;

    let metrics_array = match metrics_value {
        Value::Array(items) => items,
        _ => {
            return Err(DashboardError::InvalidStructure(
                "the 'metrics' field must be an array",
            ))
        }
    };

    metrics_array
        .iter()
        .map(Metric::from_value)
        .collect::<Result<Vec<_>, _>>()
}

fn generate(metrics: &[Metric], overrides: Option<Value>) -> Result<Value, DashboardError> {
    let mut dex = Vec::new();
    let mut compute = Vec::new();
    let mut treasury = Vec::new();
    let mut range_boost = Vec::new();
    let mut payouts = Vec::new();
    let mut advertising = Vec::new();
    let mut bridge = Vec::new();
    let mut gossip = Vec::new();
    let mut chaos = Vec::new();
    let mut tls = Vec::new();
    let mut dependency = Vec::new();
    let mut other = Vec::new();

    for metric in metrics.iter().filter(|m| !m.deprecated) {
        if metric.name == DEP_STATUS_EXPR {
            dependency.push(build_dependency_status_panel(metric));
            continue;
        }
        if metric.name == DEP_COUNTS_EXPR {
            dependency.push(build_dependency_counts_panel(metric));
            continue;
        }
        if metric.name == DEP_VIOLATION_TOTAL_EXPR {
            dependency.push(build_dependency_violation_total_panel(metric));
            continue;
        }
        if metric.name == DEP_VIOLATION_EXPR {
            dependency.push(build_dependency_violation_panel(metric));
            continue;
        }
        if metric.name == "ad_selection_attestation_total" {
            advertising.push(build_grouped_delta_panel(
                AD_ATTESTATION_PANEL_TITLE,
                AD_ATTESTATION_EXPR,
                AD_ATTESTATION_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "ad_selection_proof_verify_seconds" {
            advertising.push(build_histogram_panel(
                AD_PROOF_LATENCY_PANEL_TITLE,
                AD_PROOF_LATENCY_EXPR,
                AD_PROOF_LATENCY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "ad_budget_progress" {
            advertising.push(build_ad_timeseries_panel(
                AD_BUDGET_PROGRESS_PANEL_TITLE,
                AD_BUDGET_PROGRESS_EXPR,
                AD_BUDGET_PROGRESS_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "ad_budget_shadow_price_spike_total" {
            advertising.push(build_grouped_delta_panel(
                AD_BUDGET_SHADOW_PANEL_TITLE,
                AD_BUDGET_SHADOW_EXPR,
                AD_BUDGET_SHADOW_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "ad_budget_dual_price" {
            advertising.push(build_ad_timeseries_panel(
                AD_DUAL_PRICE_PANEL_TITLE,
                AD_DUAL_PRICE_EXPR,
                AD_DUAL_PRICE_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_COUNT_EXPR {
            treasury.push(build_treasury_status_panel(
                TREASURY_COUNT_PANEL_TITLE,
                TREASURY_COUNT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_AMOUNT_EXPR {
            treasury.push(build_treasury_status_panel(
                TREASURY_AMOUNT_PANEL_TITLE,
                TREASURY_AMOUNT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_SNAPSHOT_AGE_EXPR {
            treasury.push(build_treasury_scalar_panel(
                TREASURY_SNAPSHOT_AGE_PANEL_TITLE,
                TREASURY_SNAPSHOT_AGE_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_SCHEDULED_OLDEST_EXPR {
            treasury.push(build_treasury_scalar_panel(
                TREASURY_SCHEDULED_OLDEST_PANEL_TITLE,
                TREASURY_SCHEDULED_OLDEST_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_NEXT_EPOCH_EXPR {
            treasury.push(build_treasury_scalar_panel(
                TREASURY_NEXT_EPOCH_PANEL_TITLE,
                TREASURY_NEXT_EPOCH_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == TREASURY_LEASE_RELEASED_EXPR {
            treasury.push(build_treasury_scalar_panel(
                TREASURY_LEASE_RELEASED_PANEL_TITLE,
                TREASURY_LEASE_RELEASED_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "range_boost_forwarder_fail_total" {
            range_boost.push(build_grouped_delta_panel(
                RANGE_BOOST_FORWARDER_FAIL_PANEL_TITLE,
                RANGE_BOOST_FORWARDER_FAIL_EXPR,
                RANGE_BOOST_FORWARDER_FAIL_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "range_boost_enqueue_error_total" {
            range_boost.push(build_grouped_delta_panel(
                RANGE_BOOST_ENQUEUE_ERROR_PANEL_TITLE,
                RANGE_BOOST_ENQUEUE_ERROR_EXPR,
                RANGE_BOOST_ENQUEUE_ERROR_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "range_boost_toggle_latency_seconds" {
            range_boost.push(build_histogram_panel(
                RANGE_BOOST_TOGGLE_LATENCY_PANEL_TITLE,
                RANGE_BOOST_TOGGLE_LATENCY_EXPR,
                RANGE_BOOST_TOGGLE_LATENCY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == RANGE_BOOST_QUEUE_DEPTH_EXPR {
            range_boost.push(build_simple_timeseries_panel(
                RANGE_BOOST_QUEUE_DEPTH_PANEL_TITLE,
                RANGE_BOOST_QUEUE_DEPTH_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == RANGE_BOOST_QUEUE_OLDEST_EXPR {
            range_boost.push(build_simple_timeseries_panel(
                RANGE_BOOST_QUEUE_OLDEST_PANEL_TITLE,
                RANGE_BOOST_QUEUE_OLDEST_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == CHAOS_READINESS_EXPR {
            chaos.push(build_grouped_delta_panel(
                CHAOS_READINESS_PANEL_TITLE,
                CHAOS_READINESS_EXPR,
                CHAOS_READINESS_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == CHAOS_SITE_READINESS_EXPR {
            chaos.push(build_grouped_delta_panel(
                CHAOS_SITE_READINESS_PANEL_TITLE,
                CHAOS_SITE_READINESS_EXPR,
                CHAOS_SITE_READINESS_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "chaos_sla_breach_total" {
            chaos.push(build_grouped_delta_panel(
                CHAOS_BREACH_PANEL_TITLE,
                CHAOS_BREACH_DELTA_EXPR,
                CHAOS_BREACH_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_block_payout_read_last_seen_timestamp" {
            payouts.push(build_grouped_delta_panel(
                PAYOUT_READ_LAST_SEEN_PANEL_TITLE,
                PAYOUT_READ_LAST_SEEN_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            payouts.push(build_grouped_delta_panel(
                PAYOUT_READ_STALENESS_PANEL_TITLE,
                PAYOUT_READ_STALENESS_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_block_payout_ad_last_seen_timestamp" {
            payouts.push(build_grouped_delta_panel(
                PAYOUT_AD_LAST_SEEN_PANEL_TITLE,
                PAYOUT_AD_LAST_SEEN_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            payouts.push(build_grouped_delta_panel(
                PAYOUT_AD_STALENESS_PANEL_TITLE,
                PAYOUT_AD_STALENESS_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_block_payout_read_total" {
            payouts.push(build_grouped_delta_panel(
                PAYOUT_READ_PANEL_TITLE,
                PAYOUT_READ_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_block_payout_ad_total" {
            payouts.push(build_grouped_delta_panel(
                PAYOUT_AD_PANEL_TITLE,
                PAYOUT_AD_EXPR,
                PAYOUT_ROLE_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_compute_sla_outcome_total" {
            payouts.push(build_grouped_delta_panel(
                SLA_OUTCOME_PANEL_TITLE,
                SLA_OUTCOME_EXPR,
                SLA_OUTCOME_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_compute_sla_last_seen_timestamp" {
            payouts.push(build_simple_timeseries_panel(
                SLA_FRESHNESS_PANEL_TITLE,
                SLA_FRESHNESS_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "explorer_compute_sla_poll_error_total" {
            payouts.push(build_simple_timeseries_panel(
                SLA_POLL_ERROR_PANEL_TITLE,
                SLA_POLL_ERROR_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_reward_claims_total" {
            bridge.push(build_bridge_delta_panel(
                BRIDGE_REWARD_CLAIMS_PANEL_TITLE,
                BRIDGE_REWARD_CLAIMS_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_reward_approvals_consumed_total" {
            bridge.push(build_bridge_delta_panel(
                BRIDGE_REWARD_APPROVALS_PANEL_TITLE,
                BRIDGE_REWARD_APPROVALS_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_settlement_results_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_SETTLEMENT_RESULTS_PANEL_TITLE,
                BRIDGE_SETTLEMENT_RESULTS_EXPR,
                BRIDGE_SETTLEMENT_RESULTS_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_dispute_outcomes_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_DISPUTE_OUTCOMES_PANEL_TITLE,
                BRIDGE_DISPUTE_OUTCOMES_EXPR,
                BRIDGE_DISPUTE_OUTCOMES_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_liquidity_locked_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_LIQUIDITY_LOCKED_PANEL_TITLE,
                BRIDGE_LIQUIDITY_LOCKED_EXPR,
                BRIDGE_LIQUIDITY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_liquidity_unlocked_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_LIQUIDITY_UNLOCKED_PANEL_TITLE,
                BRIDGE_LIQUIDITY_UNLOCKED_EXPR,
                BRIDGE_LIQUIDITY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_liquidity_minted_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_LIQUIDITY_MINTED_PANEL_TITLE,
                BRIDGE_LIQUIDITY_MINTED_EXPR,
                BRIDGE_LIQUIDITY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_liquidity_burned_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_LIQUIDITY_BURNED_PANEL_TITLE,
                BRIDGE_LIQUIDITY_BURNED_EXPR,
                BRIDGE_LIQUIDITY_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_remediation_action_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_REMEDIATION_PANEL_TITLE,
                BRIDGE_REMEDIATION_EXPR,
                BRIDGE_REMEDIATION_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_remediation_dispatch_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_REMEDIATION_DISPATCH_PANEL_TITLE,
                BRIDGE_REMEDIATION_DISPATCH_EXPR,
                BRIDGE_REMEDIATION_DISPATCH_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_remediation_dispatch_ack_total" {
            bridge.push(build_grouped_delta_panel(
                BRIDGE_REMEDIATION_ACK_PANEL_TITLE,
                BRIDGE_REMEDIATION_ACK_EXPR,
                BRIDGE_REMEDIATION_ACK_LEGEND,
                metric,
            ));
            continue;
        }
        if metric.name == BRIDGE_REMEDIATION_ACK_LATENCY_METRIC {
            bridge.push(build_bridge_ack_latency_panel(metric));
            continue;
        }
        if metric.name == "bridge_remediation_spool_artifacts" {
            bridge.push(build_bridge_delta_panel(
                BRIDGE_REMEDIATION_SPOOL_PANEL_TITLE,
                BRIDGE_REMEDIATION_SPOOL_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_anomaly_total" {
            bridge.push(build_bridge_delta_panel(
                BRIDGE_ANOMALY_PANEL_TITLE,
                BRIDGE_ANOMALY_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "bridge_metric_delta" {
            bridge.push(build_bridge_metric_panel(
                BRIDGE_METRIC_DELTA_PANEL_TITLE,
                BRIDGE_METRIC_DELTA_EXPR,
                BRIDGE_METRIC_DELTA_DESCRIPTION,
            ));
            continue;
        }
        if metric.name == "bridge_metric_rate_per_second" {
            bridge.push(build_bridge_metric_panel(
                BRIDGE_METRIC_RATE_PANEL_TITLE,
                BRIDGE_METRIC_RATE_EXPR,
                BRIDGE_METRIC_RATE_DESCRIPTION,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_total" {
            tls.push(build_tls_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_last_seen_seconds" {
            tls.push(build_tls_last_seen_panel(metric));
            tls.push(build_tls_freshness_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_detail_fingerprint" {
            tls.push(build_tls_hash_panel(
                TLS_DETAIL_FINGERPRINT_PANEL_TITLE,
                TLS_DETAIL_FINGERPRINT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_fingerprint" {
            tls.push(build_tls_hash_panel(
                TLS_VARIABLES_FINGERPRINT_PANEL_TITLE,
                TLS_VARIABLES_FINGERPRINT_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_detail_unique_fingerprints" {
            tls.push(build_tls_unique_panel(
                TLS_DETAIL_UNIQUE_PANEL_TITLE,
                TLS_DETAIL_UNIQUE_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_unique_fingerprints" {
            tls.push(build_tls_unique_panel(
                TLS_VARIABLES_UNIQUE_PANEL_TITLE,
                TLS_VARIABLES_UNIQUE_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_detail_fingerprint_total" {
            tls.push(build_tls_fingerprint_delta_panel(
                TLS_DETAIL_FINGERPRINT_DELTA_PANEL_TITLE,
                TLS_DETAIL_FINGERPRINT_DELTA_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_variables_fingerprint_total" {
            tls.push(build_tls_fingerprint_delta_panel(
                TLS_VARIABLES_FINGERPRINT_DELTA_PANEL_TITLE,
                TLS_VARIABLES_FINGERPRINT_DELTA_EXPR,
                metric,
            ));
            continue;
        }
        if metric.name == "tls_env_warning_active_snapshots" {
            tls.push(build_tls_active_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_stale_snapshots" {
            tls.push(build_tls_stale_panel(metric));
            continue;
        }
        if metric.name == "tls_env_warning_retention_seconds" {
            tls.push(build_tls_retention_panel(metric));
            continue;
        }

        let mut panel = Map::new();
        panel.insert("type".into(), Value::from("timeseries"));
        panel.insert("title".into(), Value::from(metric.name.clone()));

        let mut target = Map::new();
        target.insert("expr".into(), Value::from(metric.name.clone()));
        panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

        let mut legend = Map::new();
        legend.insert("showLegend".into(), Value::from(false));
        let mut options = Map::new();
        options.insert("legend".into(), Value::Object(legend));
        panel.insert("options".into(), Value::Object(options));

        let mut datasource = Map::new();
        datasource.insert("type".into(), Value::from("foundation-telemetry"));
        datasource.insert("uid".into(), Value::from("foundation"));
        panel.insert("datasource".into(), Value::Object(datasource));

        let panel_value = Value::Object(panel);
        if metric.name.starts_with("dex_") {
            dex.push(panel_value);
        } else if metric.name.starts_with("compute_") || metric.name.starts_with("scheduler_") {
            compute.push(panel_value);
        } else if metric.name.starts_with("treasury_") {
            treasury.push(panel_value);
        } else if metric.name.starts_with("range_boost_") {
            range_boost.push(panel_value);
        } else if metric.name.starts_with("gossip_") {
            gossip.push(panel_value);
        } else {
            other.push(panel_value);
        }
    }

    let mut panels = Vec::new();
    let mut next_id: u64 = 1;

    for (title, mut entries) in [
        ("DEX", dex),
        ("Compute", compute),
        ("Treasury", treasury),
        (RANGE_BOOST_ROW_TITLE, range_boost),
        (PAYOUT_ROW_TITLE, payouts),
        (AD_ROW_TITLE, advertising),
        ("Bridge", bridge),
        (CHAOS_ROW_TITLE, chaos),
        ("Gossip", gossip),
        ("TLS", tls),
        ("Dependencies", dependency),
        ("Other", other),
    ] {
        if entries.is_empty() {
            continue;
        }
        let mut row = Map::new();
        row.insert("type".into(), Value::from("row"));
        row.insert("title".into(), Value::from(title));
        row.insert("id".into(), Value::from(next_id));
        next_id += 1;
        panels.push(Value::Object(row));

        for entry in entries.iter_mut() {
            if let Value::Object(ref mut map) = entry {
                map.insert("id".into(), Value::from(next_id));
            }
            next_id += 1;
        }
        panels.extend(entries);
    }

    let mut dashboard_map = Map::new();
    dashboard_map.insert("title".into(), Value::from("The-Block Auto"));
    dashboard_map.insert("schemaVersion".into(), Value::from(37u64));
    dashboard_map.insert("version".into(), Value::from(1u64));
    dashboard_map.insert("panels".into(), Value::Array(panels));
    let mut dashboard = Value::Object(dashboard_map);

    if let Some(override_value) = overrides {
        apply_overrides(&mut dashboard, override_value)?;
    }

    Ok(dashboard)
}

fn build_bridge_delta_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(false));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_bridge_metric_panel(title: &str, expr: &str, description: &str) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !description.is_empty() {
        panel.insert("description".into(), Value::from(description));
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_simple_timeseries_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(false));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_histogram_panel(title: &str, expr: &str, legend_format: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from(legend_format));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    legend.insert("displayMode".into(), Value::from("table"));
    legend.insert("placement".into(), Value::from("right"));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_ad_timeseries_panel(
    title: &str,
    expr: &str,
    legend_format: &str,
    metric: &Metric,
) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from(legend_format));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_grouped_delta_panel(
    title: &str,
    expr: &str,
    legend_format: &str,
    metric: &Metric,
) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from(legend_format));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_bridge_ack_latency_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert(
        "title".into(),
        Value::from(BRIDGE_REMEDIATION_ACK_LATENCY_PANEL_TITLE),
    );
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let quantiles = [(0.5, "p50"), (0.95, "p95")];
    let mut targets = Vec::new();
    for (quantile, label) in quantiles {
        let expr = format!(
            "histogram_quantile({:.2}, sum by (le, playbook, state)(rate({}_bucket[5m])))",
            quantile, BRIDGE_REMEDIATION_ACK_LATENCY_METRIC
        );
        let mut target = Map::new();
        target.insert("expr".into(), Value::from(expr));
        target.insert(
            "legendFormat".into(),
            Value::from(format!("{{playbook}} · {{state}} · {}", label)),
        );
        targets.push(Value::Object(target));
    }
    for phase in ["retry", "escalate"] {
        let mut target = Map::new();
        target.insert(
            "expr".into(),
            Value::from(format!(
                "bridge_remediation_ack_target_seconds{{phase=\"{}\"}}",
                phase
            )),
        );
        target.insert(
            "legendFormat".into(),
            Value::from(format!("{{playbook}} · {} target", phase)),
        );
        targets.push(Value::Object(target));
    }
    panel.insert("targets".into(), Value::Array(targets));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_PANEL_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_dependency_status_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(DEP_STATUS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(DEP_STATUS_EXPR));
    target.insert(
        "legendFormat".into(),
        Value::from("{{status}} · {{detail}}"),
    );
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_dependency_counts_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(DEP_COUNTS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(DEP_COUNTS_EXPR));
    target.insert("legendFormat".into(), Value::from("{{kind}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_dependency_violation_total_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(DEP_VIOLATION_TOTAL_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(DEP_VIOLATION_TOTAL_EXPR));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(false));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_dependency_violation_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(DEP_VIOLATION_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(DEP_VIOLATION_EXPR));
    target.insert(
        "legendFormat".into(),
        Value::from("{{crate}} {{version}} · {{kind}}"),
    );
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_treasury_status_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from("{{status}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_treasury_scalar_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from("{{__name__}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_last_seen_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_LAST_SEEN_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_LAST_SEEN_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_freshness_panel(metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(TLS_FRESHNESS_PANEL_TITLE));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(TLS_FRESHNESS_EXPR));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_active_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_ACTIVE_PANEL_TITLE, TLS_ACTIVE_EXPR, metric)
}

fn build_tls_stale_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_STALE_PANEL_TITLE, TLS_STALE_EXPR, metric)
}

fn build_tls_retention_panel(metric: &Metric) -> Value {
    build_tls_scalar_panel(TLS_RETENTION_PANEL_TITLE, TLS_RETENTION_EXPR, metric)
}

fn build_tls_hash_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert("legendFormat".into(), Value::from("{{prefix}} · {{code}}"));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_unique_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    build_tls_hash_panel(title, expr, metric)
}

fn build_tls_fingerprint_delta_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    target.insert(
        "legendFormat".into(),
        Value::from("{{prefix}} · {{code}} · {{fingerprint}}"),
    );
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(true));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

fn build_tls_scalar_panel(title: &str, expr: &str, metric: &Metric) -> Value {
    let mut panel = Map::new();
    panel.insert("type".into(), Value::from("timeseries"));
    panel.insert("title".into(), Value::from(title));
    if !metric.description.is_empty() {
        panel.insert(
            "description".into(),
            Value::from(metric.description.clone()),
        );
    }

    let mut target = Map::new();
    target.insert("expr".into(), Value::from(expr));
    panel.insert("targets".into(), Value::Array(vec![Value::Object(target)]));

    let mut legend = Map::new();
    legend.insert("showLegend".into(), Value::from(false));
    let mut options = Map::new();
    options.insert("legend".into(), Value::Object(legend));
    panel.insert("options".into(), Value::Object(options));

    let mut datasource = Map::new();
    datasource.insert("type".into(), Value::from("foundation-telemetry"));
    datasource.insert("uid".into(), Value::from("foundation"));
    panel.insert("datasource".into(), Value::Object(datasource));

    Value::Object(panel)
}

/// Parse a Prometheus text-format payload into a metric/value map.
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_prometheus_snapshot(payload: &str) -> MetricSnapshot {
    let mut values = MetricSnapshot::new();
    for line in payload.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        if let Some(name) = parts.next() {
            if name.ends_with("_bucket") || name.ends_with("_count") || name.ends_with("_sum") {
                continue;
            }
            if let Some(value_str) = parts.last() {
                if let Some(value) = parse_metric_value(value_str) {
                    values.insert(name.to_string(), value);
                }
            }
        }
    }
    values
}

fn parse_metric_value(value: &str) -> Option<MetricValue> {
    if let Ok(int) = value.parse::<i64>() {
        return Some(MetricValue::Integer(int));
    }
    if let Ok(uint) = value.parse::<u64>() {
        return Some(MetricValue::Unsigned(uint));
    }
    value.parse::<f64>().ok().map(MetricValue::Float)
}

/// Render the in-house HTML snapshot used by the telemetry dashboard helpers.
#[cfg_attr(not(test), allow(dead_code))]
pub fn render_html_snapshot(
    endpoint: &str,
    metrics: &[Metric],
    snapshot: &MetricSnapshot,
) -> String {
    let mut sections = [
        ("DEX", Vec::new()),
        ("Compute", Vec::new()),
        ("Treasury", Vec::new()),
        ("Gossip", Vec::new()),
        ("Other", Vec::new()),
    ];

    for metric in metrics.iter().filter(|metric| !metric.deprecated) {
        let bucket = match categorize_metric(metric) {
            MetricCategory::Dex => 0,
            MetricCategory::Compute => 1,
            MetricCategory::Treasury => 2,
            MetricCategory::Gossip => 3,
            MetricCategory::Other => 4,
        };
        sections[bucket].1.push(metric);
    }

    let mut rendered_sections = Vec::new();
    for (title, metrics) in sections.into_iter() {
        let section = render_section(title, &metrics, snapshot);
        if !section.is_empty() {
            rendered_sections.push(section);
        }
    }

    let body = if rendered_sections.is_empty() {
        "<p>No metrics available.</p>".to_string()
    } else {
        rendered_sections.join("\n")
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\">\n    <meta http-equiv=\"refresh\" content=\"5\">\n    <title>The-Block Telemetry Snapshot</title>\n    <style>\n      body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #0f1115; color: #f8fafc; }}\n      h1 {{ margin-bottom: 1rem; }}\n      table {{ width: 100%; border-collapse: collapse; margin-bottom: 2rem; }}\n      th, td {{ border-bottom: 1px solid #1f2937; padding: 0.5rem 0.75rem; text-align: left; }}\n      th {{ text-transform: uppercase; font-size: 0.75rem; letter-spacing: 0.08em; color: #94a3b8; }}\n      tr.metric-row:hover {{ background: rgba(148, 163, 184, 0.08); }}\n      .status {{ font-weight: 600; }}\n      .error {{ color: #fca5a5; }}\n    </style>\n  </head>\n  <body>\n    <h1>The-Block Telemetry Snapshot</h1>\n    <p class=\"status\">Source: {}</p>\n    {}\n  </body>\n</html>\n",
        html_escape(endpoint),
        body
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn render_section(title: &str, metrics: &[&Metric], snapshot: &MetricSnapshot) -> String {
    if metrics.is_empty() {
        return String::new();
    }
    let mut rows = String::new();
    for metric in metrics {
        let name = html_escape(&metric.name);
        let description = if metric.description.is_empty() {
            "&mdash;".to_string()
        } else {
            html_escape(&metric.description)
        };
        let value_display = match snapshot.get(&metric.name) {
            Some(value) => format_metric_value(value),
            None => "<span class=\"error\">missing</span>".to_string(),
        };
        rows.push_str(&format!(
            "      <tr class=\"metric-row\"><td><code>{name}</code></td><td>{description}</td><td>{}</td></tr>\n",
            value_display
        ));
    }

    format!(
        "<h2>{}</h2>\n<table>\n  <thead>\n    <tr><th>Metric</th><th>Description</th><th>Value</th></tr>\n  </thead>\n  <tbody>\n{}  </tbody>\n</table>",
        html_escape(title),
        rows
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg_attr(not(test), allow(dead_code))]
fn format_metric_value(value: &MetricValue) -> String {
    match value {
        MetricValue::Float(v) => format_float(*v),
        MetricValue::Integer(v) => v.to_string(),
        MetricValue::Unsigned(v) => v.to_string(),
    }
}

fn format_float(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let mut formatted = format!("{value:.6}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted.is_empty() {
        formatted.push('0');
    }
    formatted
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum MetricCategory {
    Dex,
    Compute,
    Treasury,
    Gossip,
    Other,
}

#[cfg_attr(not(test), allow(dead_code))]
fn categorize_metric(metric: &Metric) -> MetricCategory {
    let name = metric.name.as_str();
    if name.starts_with("dex_") {
        MetricCategory::Dex
    } else if name.starts_with("compute_") || name.starts_with("scheduler_") {
        MetricCategory::Compute
    } else if name.starts_with("treasury_") {
        MetricCategory::Treasury
    } else if name.starts_with("gossip_") {
        MetricCategory::Gossip
    } else {
        MetricCategory::Other
    }
}

pub(crate) fn apply_overrides(base: &mut Value, overrides: Value) -> Result<(), DashboardError> {
    let Value::Object(ref mut base_map) = base else {
        return Err(DashboardError::InvalidStructure(
            "generated dashboard must be a JSON object",
        ));
    };

    let Value::Object(override_map) = overrides else {
        return Err(DashboardError::InvalidStructure(
            "dashboard overrides must be a JSON object",
        ));
    };

    for (key, value) in override_map {
        base_map.insert(key, value);
    }
    Ok(())
}

pub fn render_pretty(dashboard: &Value) -> Result<String, foundation_serialization::Error> {
    let mut rendered = json::to_string_pretty(dashboard)?;
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_metrics_by_prefix() {
        let metrics = vec![
            Metric {
                name: "dex_trades_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "compute_jobs_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "gossip_peers".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "storage_reads".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        // Four rows + four panels should yield eight entries.
        assert_eq!(panels.len(), 8);

        let titles: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map) => {
                    if matches!(map.get("type"), Some(Value::String(t)) if t == "row") {
                        match map.get("title") {
                            Some(Value::String(title)) => Some(title.as_str()),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        assert_eq!(titles, vec!["DEX", "Compute", "Gossip", "Other"]);
    }

    #[test]
    fn payouts_row_includes_grouped_panels() {
        let metrics = vec![
            Metric {
                name: "explorer_block_payout_read_total".into(),
                description: "Read subsidy payouts".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "explorer_block_payout_ad_total".into(),
                description: "Advertising payouts".into(),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 3);

        let row = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("payout row present");
        assert_eq!(row.get("title"), Some(&Value::from(PAYOUT_ROW_TITLE)));

        let read_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == PAYOUT_READ_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("read payout panel present");
        assert_eq!(
            read_panel.get("description"),
            Some(&Value::from("Read subsidy payouts"))
        );
        let read_target = read_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| target.as_object())
            .expect("read target object");
        assert_eq!(
            read_target.get("expr"),
            Some(&Value::from(PAYOUT_READ_EXPR))
        );
        assert_eq!(
            read_target.get("legendFormat"),
            Some(&Value::from(PAYOUT_ROLE_LEGEND))
        );
        let read_options = read_panel
            .get("options")
            .and_then(Value::as_object)
            .and_then(|opts| opts.get("legend"))
            .and_then(Value::as_object)
            .and_then(|legend| legend.get("showLegend"))
            .and_then(Value::as_bool)
            .expect("legend flag");
        assert!(read_options);

        let ad_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == PAYOUT_AD_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("advertising payout panel present");
        let ad_target = ad_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| target.as_object())
            .expect("ad target object");
        assert_eq!(ad_target.get("expr"), Some(&Value::from(PAYOUT_AD_EXPR)));
        assert_eq!(
            ad_target.get("legendFormat"),
            Some(&Value::from(PAYOUT_ROLE_LEGEND))
        );
    }

    #[test]
    fn tls_panel_is_included_with_custom_query() {
        let metrics = vec![Metric {
            name: "tls_env_warning_total".into(),
            description: "TLS warnings grouped by prefix/code".into(),
            unit: String::new(),
            deprecated: false,
        }];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 2);

        let rows: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    map.get("title").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows, vec!["TLS"]);

        let tls_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("tls panel present");

        let expr = tls_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("tls panel expression");
        assert_eq!(expr, super::TLS_PANEL_EXPR);

        let legend_enabled = tls_panel
            .get("options")
            .and_then(|options| match options {
                Value::Object(map) => map.get("legend"),
                _ => None,
            })
            .and_then(|legend| match legend {
                Value::Object(map) => map.get("showLegend"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(legend_enabled, "TLS panel legend should be enabled");

        let legend_format = tls_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("legendFormat"),
                _ => None,
            })
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(legend_format, "{{prefix}} · {{code}}");
    }

    #[test]
    fn treasury_metrics_render_in_dedicated_row() {
        let metrics = vec![Metric {
            name: TREASURY_COUNT_EXPR.into(),
            description: "Treasury disbursements grouped by status".into(),
            unit: String::new(),
            deprecated: false,
        }];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 2);

        let row = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("treasury row present");
        assert_eq!(row.get("title"), Some(&Value::from("Treasury")));

        let panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == TREASURY_COUNT_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("treasury panel present");

        let legend_enabled = panel
            .get("options")
            .and_then(|value| match value {
                Value::Object(map) => map.get("legend"),
                _ => None,
            })
            .and_then(|value| match value {
                Value::Object(map) => map.get("showLegend"),
                _ => None,
            });
        assert_eq!(legend_enabled, Some(&Value::from(true)));

        let legend_format = panel
            .get("targets")
            .and_then(|value| match value {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|value| match value {
                Value::Object(map) => map.get("legendFormat"),
                _ => None,
            });
        assert_eq!(legend_format, Some(&Value::from("{{status}}")));
    }

    #[test]
    fn range_boost_metrics_render_in_row() {
        let metrics = vec![
            Metric {
                name: "range_boost_forwarder_fail_total".into(),
                description: "RangeBoost forwarder failures observed".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "range_boost_enqueue_error_total".into(),
                description: "RangeBoost enqueue attempts dropped due to injection".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "range_boost_toggle_latency_seconds".into(),
                description: "Latency observed between RangeBoost enable/disable toggles".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: RANGE_BOOST_QUEUE_DEPTH_EXPR.into(),
                description: "Current number of bundles pending in the RangeBoost queue".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: RANGE_BOOST_QUEUE_OLDEST_EXPR.into(),
                description: "Age in seconds of the oldest RangeBoost queue entry".into(),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(
            panels.len(),
            metrics.len() + 1,
            "expected Range Boost row and panels"
        );

        let row = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("range boost row present");
        assert_eq!(row.get("title"), Some(&Value::from(RANGE_BOOST_ROW_TITLE)));

        let assert_panel = |title: &str, expr: &str, legend: Option<&str>| {
            let panel = panels
                .iter()
                .find_map(|panel| match panel {
                    Value::Object(map)
                        if map
                            .get("title")
                            .and_then(Value::as_str)
                            .map(|candidate| candidate == title)
                            .unwrap_or(false) =>
                    {
                        Some(map)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing panel '{title}'"));

            let expr_value = panel
                .get("targets")
                .and_then(|value| match value {
                    Value::Array(items) => items.first(),
                    _ => None,
                })
                .and_then(|value| match value {
                    Value::Object(map) => map.get("expr"),
                    _ => None,
                })
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert_eq!(expr_value, expr);

            let legend_value = panel
                .get("targets")
                .and_then(|value| match value {
                    Value::Array(items) => items.first(),
                    _ => None,
                })
                .and_then(|value| match value {
                    Value::Object(map) => map.get("legendFormat"),
                    _ => None,
                })
                .and_then(Value::as_str);
            match legend {
                Some(expected) => assert_eq!(legend_value, Some(expected)),
                None => assert!(legend_value.is_none()),
            }
        };

        assert_panel(
            RANGE_BOOST_FORWARDER_FAIL_PANEL_TITLE,
            RANGE_BOOST_FORWARDER_FAIL_EXPR,
            Some(RANGE_BOOST_FORWARDER_FAIL_LEGEND),
        );
        assert_panel(
            RANGE_BOOST_ENQUEUE_ERROR_PANEL_TITLE,
            RANGE_BOOST_ENQUEUE_ERROR_EXPR,
            Some(RANGE_BOOST_ENQUEUE_ERROR_LEGEND),
        );
        assert_panel(
            RANGE_BOOST_TOGGLE_LATENCY_PANEL_TITLE,
            RANGE_BOOST_TOGGLE_LATENCY_EXPR,
            Some(RANGE_BOOST_TOGGLE_LATENCY_LEGEND),
        );
        assert_panel(
            RANGE_BOOST_QUEUE_DEPTH_PANEL_TITLE,
            RANGE_BOOST_QUEUE_DEPTH_EXPR,
            None,
        );
        assert_panel(
            RANGE_BOOST_QUEUE_OLDEST_PANEL_TITLE,
            RANGE_BOOST_QUEUE_OLDEST_EXPR,
            None,
        );
    }

    #[test]
    fn bridge_metrics_render_in_dedicated_row() {
        let metrics = vec![
            Metric {
                name: "bridge_reward_claims_total".into(),
                description: "Bridge reward claim operations".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_reward_approvals_consumed_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_settlement_results_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_dispute_outcomes_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_liquidity_locked_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_liquidity_unlocked_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_liquidity_minted_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_liquidity_burned_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_anomaly_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_remediation_action_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_remediation_dispatch_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_remediation_dispatch_ack_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_remediation_ack_latency_seconds".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_remediation_spool_artifacts".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_metric_delta".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "bridge_metric_rate_per_second".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 17);

        let row = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("bridge row present");
        assert_eq!(row.get("title"), Some(&Value::from("Bridge")));

        let claims_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REWARD_CLAIMS_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("claims panel present");
        let claims_expr = claims_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("claims expression");
        assert_eq!(claims_expr, BRIDGE_REWARD_CLAIMS_EXPR);

        let dispatch_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REMEDIATION_DISPATCH_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("dispatch panel present");
        let dispatch_expr = dispatch_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("dispatch expression");
        assert_eq!(dispatch_expr, BRIDGE_REMEDIATION_DISPATCH_EXPR);

        let ack_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REMEDIATION_ACK_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("dispatch acknowledgement panel present");
        let ack_expr = ack_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("dispatch acknowledgement expression");
        assert_eq!(ack_expr, BRIDGE_REMEDIATION_ACK_EXPR);
        let dispatch_legend = dispatch_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("legendFormat"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("dispatch legend format");
        assert_eq!(dispatch_legend, BRIDGE_REMEDIATION_DISPATCH_LEGEND);

        let ack_latency_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REMEDIATION_ACK_LATENCY_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("ack latency panel present");
        let latency_targets = ack_latency_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => Some(items),
                _ => None,
            })
            .expect("ack latency targets");
        assert_eq!(latency_targets.len(), 4);
        let latency_expr_first = latency_targets
            .get(0)
            .and_then(Value::as_object)
            .and_then(|map| map.get("expr"))
            .and_then(Value::as_str)
            .expect("ack latency p50 expr");
        assert_eq!(
            latency_expr_first,
            "histogram_quantile(0.50, sum by (le, playbook, state)(rate(bridge_remediation_ack_latency_seconds_bucket[5m])))"
        );
        let latency_legend_first = latency_targets
            .get(0)
            .and_then(Value::as_object)
            .and_then(|map| map.get("legendFormat"))
            .and_then(Value::as_str)
            .expect("ack latency p50 legend");
        assert_eq!(latency_legend_first, "{playbook} · {state} · p50");
        let latency_expr_second = latency_targets
            .get(1)
            .and_then(Value::as_object)
            .and_then(|map| map.get("expr"))
            .and_then(Value::as_str)
            .expect("ack latency p95 expr");
        assert_eq!(
            latency_expr_second,
            "histogram_quantile(0.95, sum by (le, playbook, state)(rate(bridge_remediation_ack_latency_seconds_bucket[5m])))"
        );
        let latency_legend_second = latency_targets
            .get(1)
            .and_then(Value::as_object)
            .and_then(|map| map.get("legendFormat"))
            .and_then(Value::as_str)
            .expect("ack latency p95 legend");
        assert_eq!(latency_legend_second, "{playbook} · {state} · p95");

        let retry_target = latency_targets
            .get(2)
            .and_then(Value::as_object)
            .expect("ack latency retry target");
        assert_eq!(
            retry_target
                .get("expr")
                .and_then(Value::as_str)
                .expect("retry target expr"),
            "bridge_remediation_ack_target_seconds{phase=\"retry\"}"
        );
        assert_eq!(
            retry_target
                .get("legendFormat")
                .and_then(Value::as_str)
                .expect("retry target legend"),
            "{playbook} · retry target"
        );

        let escalate_target = latency_targets
            .get(3)
            .and_then(Value::as_object)
            .expect("ack latency escalate target");
        assert_eq!(
            escalate_target
                .get("expr")
                .and_then(Value::as_str)
                .expect("escalate target expr"),
            "bridge_remediation_ack_target_seconds{phase=\"escalate\"}"
        );
        assert_eq!(
            escalate_target
                .get("legendFormat")
                .and_then(Value::as_str)
                .expect("escalate target legend"),
            "{playbook} · escalate target"
        );

        let spool_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REMEDIATION_SPOOL_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("spool artifact panel present");
        let spool_expr = spool_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("spool artifact expression");
        assert_eq!(spool_expr, BRIDGE_REMEDIATION_SPOOL_EXPR);
        let spool_legend = spool_panel
            .get("options")
            .and_then(Value::as_object)
            .and_then(|options| options.get("legend"))
            .and_then(Value::as_object)
            .and_then(|legend| legend.get("showLegend"))
            .and_then(Value::as_bool);
        assert_eq!(spool_legend, Some(false));

        let delta_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_METRIC_DELTA_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("delta panel present");
        let delta_expr = delta_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("delta expression");
        assert_eq!(delta_expr, BRIDGE_METRIC_DELTA_EXPR);
        let delta_description = delta_panel
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(delta_description, BRIDGE_METRIC_DELTA_DESCRIPTION);
        let delta_legend = delta_panel
            .get("options")
            .and_then(|value| match value {
                Value::Object(map) => map.get("legend"),
                _ => None,
            })
            .and_then(|value| match value {
                Value::Object(map) => map.get("showLegend"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(delta_legend, "delta legend enabled");

        let locked_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_LIQUIDITY_LOCKED_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("locked liquidity panel present");
        let locked_target = locked_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .expect("locked target");
        assert_eq!(
            locked_target.get("expr"),
            Some(&Value::from(BRIDGE_LIQUIDITY_LOCKED_EXPR))
        );
        assert_eq!(
            locked_target.get("legendFormat"),
            Some(&Value::from(BRIDGE_LIQUIDITY_LEGEND))
        );

        let remediation_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_REMEDIATION_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("remediation panel present");
        let remediation_target = remediation_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .expect("remediation target");
        assert_eq!(
            remediation_target.get("expr"),
            Some(&Value::from(BRIDGE_REMEDIATION_EXPR))
        );
        assert_eq!(
            remediation_target.get("legendFormat"),
            Some(&Value::from(BRIDGE_REMEDIATION_LEGEND))
        );

        let anomaly_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_ANOMALY_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("anomaly panel present");
        let anomaly_expr = anomaly_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("anomaly expression");
        assert_eq!(anomaly_expr, BRIDGE_ANOMALY_EXPR);

        let rate_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_METRIC_RATE_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("rate panel present");
        let rate_expr = rate_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("rate expression");
        assert_eq!(rate_expr, BRIDGE_METRIC_RATE_EXPR);
        let rate_description = rate_panel
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(rate_description, BRIDGE_METRIC_RATE_DESCRIPTION);
        let rate_legend = rate_panel
            .get("options")
            .and_then(|value| match value {
                Value::Object(map) => map.get("legend"),
                _ => None,
            })
            .and_then(|value| match value {
                Value::Object(map) => map.get("showLegend"),
                _ => None,
            })
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(rate_legend, "rate legend enabled");

        let dispute_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == BRIDGE_DISPUTE_OUTCOMES_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("dispute panel present");
        let dispute_legend = dispute_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("legendFormat"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("dispute legend format");
        assert_eq!(dispute_legend, BRIDGE_DISPUTE_OUTCOMES_LEGEND);
    }

    #[test]
    fn tls_last_seen_and_freshness_panels_are_included() {
        let metrics = vec![Metric {
            name: "tls_env_warning_last_seen_seconds".into(),
            description: String::from("TLS warning freshness"),
            unit: String::new(),
            deprecated: false,
        }];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 3);

        let last_seen_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_LAST_SEEN_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("last seen panel present");
        let last_seen_expr = last_seen_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("last seen expr");
        assert_eq!(last_seen_expr, super::TLS_LAST_SEEN_EXPR);

        let freshness_panel = panels
            .iter()
            .find_map(|panel| match panel {
                Value::Object(map)
                    if map
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| title == super::TLS_FRESHNESS_PANEL_TITLE)
                        .unwrap_or(false) =>
                {
                    Some(map)
                }
                _ => None,
            })
            .expect("freshness panel present");
        let freshness_expr = freshness_panel
            .get("targets")
            .and_then(|targets| match targets {
                Value::Array(items) => items.first(),
                _ => None,
            })
            .and_then(|target| match target {
                Value::Object(map) => map.get("expr"),
                _ => None,
            })
            .and_then(Value::as_str)
            .expect("freshness expr");
        assert_eq!(freshness_expr, super::TLS_FRESHNESS_EXPR);
    }

    #[test]
    fn tls_status_scalar_panels_are_included() {
        let metrics = vec![
            Metric {
                name: "tls_env_warning_active_snapshots".into(),
                description: String::from("Active TLS warning snapshots"),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "tls_env_warning_stale_snapshots".into(),
                description: String::from("Stale TLS warning snapshots"),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "tls_env_warning_retention_seconds".into(),
                description: String::from("Configured retention window"),
                unit: String::new(),
                deprecated: false,
            },
        ];

        let dashboard = generate(&metrics, None).expect("dashboard generation");
        let panels = match &dashboard {
            Value::Object(map) => match map.get("panels") {
                Some(Value::Array(items)) => items,
                _ => panic!("panels missing"),
            },
            _ => panic!("dashboard is not an object"),
        };

        assert_eq!(panels.len(), 4);

        let titles: Vec<&str> = panels
            .iter()
            .filter_map(|panel| match panel {
                Value::Object(map)
                    if matches!(map.get("type"), Some(Value::String(kind)) if kind == "row") =>
                {
                    None
                }
                Value::Object(map) => map.get("title").and_then(Value::as_str),
                _ => None,
            })
            .collect();

        assert!(titles.contains(&super::TLS_ACTIVE_PANEL_TITLE));
        assert!(titles.contains(&super::TLS_STALE_PANEL_TITLE));
        assert!(titles.contains(&super::TLS_RETENTION_PANEL_TITLE));
    }

    #[test]
    fn applies_overrides() {
        let metrics = vec![Metric {
            name: "dex_volume".into(),
            description: String::new(),
            unit: String::new(),
            deprecated: false,
        }];

        let overrides = json::value_from_str(r#"{"title":"Custom"}"#).unwrap();
        let dashboard = generate(&metrics, Some(overrides)).expect("dashboard");
        let title = match dashboard {
            Value::Object(map) => match map.get("title") {
                Some(Value::String(text)) => Some(text.clone()),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(title.as_deref(), Some("Custom"));
    }

    #[test]
    fn rejects_invalid_metric() {
        let value =
            json::value_from_str(r#"{"metrics":[{"description":"missing name"}]}"#).unwrap();
        let error = extract_metrics(&value).expect_err("missing name should fail");
        match error {
            DashboardError::InvalidMetric { field, .. } => assert_eq!(field, "name"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_prometheus_snapshot_skips_histograms() {
        let payload = r#"
# HELP dex_trades_total Total trades
# TYPE dex_trades_total counter
dex_trades_total 42
scheduler_match_total_bucket{result="ok",le="0.5"} 1
scheduler_match_total_sum 10
"#;
        let parsed = parse_prometheus_snapshot(payload);
        assert_eq!(
            parsed.get("dex_trades_total"),
            Some(&MetricValue::Integer(42))
        );
        assert!(!parsed.contains_key("scheduler_match_total_bucket"));
        assert!(!parsed.contains_key("scheduler_match_total_sum"));
    }

    #[test]
    fn render_html_snapshot_renders_sections() {
        let metrics = vec![
            Metric {
                name: "dex_trades_total".into(),
                description: "Total DEX trades".into(),
                unit: String::new(),
                deprecated: false,
            },
            Metric {
                name: "compute_jobs_total".into(),
                description: String::new(),
                unit: String::new(),
                deprecated: false,
            },
        ];
        let mut snapshot = MetricSnapshot::new();
        snapshot.insert("dex_trades_total".to_string(), MetricValue::Integer(42));
        let html = render_html_snapshot("http://localhost:9090/metrics", &metrics, &snapshot);
        assert!(html.contains("The-Block Telemetry Snapshot"));
        assert!(html.contains("<h2>DEX</h2>"));
        assert!(html.contains("<h2>Compute</h2>"));
        assert!(html.contains("dex_trades_total"));
        assert!(html.contains("42"));
        assert!(html.contains("missing"));
    }

    #[test]
    fn format_metric_value_trims_trailing_zeroes() {
        assert_eq!(format_metric_value(&MetricValue::Float(42.0)), "42");
        assert_eq!(format_metric_value(&MetricValue::Float(3.140000)), "3.14");
        assert_eq!(format_metric_value(&MetricValue::Float(0.0)), "0");
        assert_eq!(
            format_metric_value(&MetricValue::Float(f64::INFINITY)),
            "inf"
        );
    }
}
