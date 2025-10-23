use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

const ALERT_FILE: &str = "alert.rules.yml";

const GROUP_BRIDGE: &str = "bridge";
const GROUP_CHAIN_HEALTH: &str = "chain-health";
const GROUP_DEPENDENCY_REGISTRY: &str = "dependency-registry";
const GROUP_TREASURY: &str = "treasury";

const ALERT_DELTA: &str = "BridgeCounterDeltaSkew";
const ALERT_RATE: &str = "BridgeCounterRateSkew";
const ALERT_DELTA_LABEL: &str = "BridgeCounterDeltaLabelSkew";
const ALERT_RATE_LABEL: &str = "BridgeCounterRateLabelSkew";

const ALERT_CONVERGENCE_LAG: &str = "ConvergenceLag";
const ALERT_CONSUMER_FEE: &str = "ConsumerFeeComfortBreached";
const ALERT_DEFERRAL_RATIO: &str = "IndustrialDeferralRatioHigh";
const ALERT_SUBSIDY_SPIKE: &str = "SubsidyGrowthSpike";
const ALERT_RENT_ESCROW: &str = "RentEscrowLockedSpike";
const ALERT_TLS_BURST: &str = "TlsEnvWarningBurst";
const ALERT_TLS_NEW_DETAIL: &str = "TlsEnvWarningNewDetailFingerprint";
const ALERT_TLS_NEW_VARIABLES: &str = "TlsEnvWarningNewVariablesFingerprint";
const ALERT_TLS_DETAIL_FLOOD: &str = "TlsEnvWarningDetailFingerprintFlood";
const ALERT_TLS_VARIABLES_FLOOD: &str = "TlsEnvWarningVariablesFingerprintFlood";
const ALERT_TLS_STALE: &str = "TlsEnvWarningSnapshotsStale";

const ALERT_DEP_DRIFT: &str = "DependencyRegistryDriftDetected";
const ALERT_DEP_VIOLATIONS: &str = "DependencyRegistryPolicyViolations";
const ALERT_DEP_BASELINE: &str = "DependencyRegistryBaselineError";
const ALERT_DEP_VIOLATION_SPIKE: &str = "DependencyPolicyViolationSpike";

const ALERT_TREASURY_SNAPSHOT: &str = "TreasuryDisbursementSnapshotStale";
const ALERT_TREASURY_OVERDUE: &str = "TreasuryDisbursementScheduleOverdue";

const EXPECTED_DELTA_EXPR: &str = "(bridge_metric_delta>avg_over_time(bridge_metric_delta[30m])*3)and(bridge_metric_delta>10)and(count_over_time(bridge_metric_delta[30m])>=6)";
const EXPECTED_RATE_EXPR: &str = "(bridge_metric_rate_per_second>avg_over_time(bridge_metric_rate_per_second[30m])*3)and(bridge_metric_rate_per_second>0.5)and(count_over_time(bridge_metric_rate_per_second[30m])>=6)";
const EXPECTED_DELTA_LABEL_EXPR: &str = "(bridge_metric_delta{labels!=\"\"}>avg_over_time(bridge_metric_delta{labels!=\"\"}[30m])*3)and(bridge_metric_delta{labels!=\"\"}>5)and(count_over_time(bridge_metric_delta{labels!=\"\"}[30m])>=6)";
const EXPECTED_RATE_LABEL_EXPR: &str = "(bridge_metric_rate_per_second{labels!=\"\"}>avg_over_time(bridge_metric_rate_per_second{labels!=\"\"}[30m])*3)and(bridge_metric_rate_per_second{labels!=\"\"}>0.25)and(count_over_time(bridge_metric_rate_per_second{labels!=\"\"}[30m])>=6)";

const EXPECTED_CONVERGENCE_EXPR: &str = "histogram_quantile(0.95,sum(rate(gossip_convergence_seconds_bucket[5m]))by(le))>30";
const EXPECTED_CONSUMER_FEE_EXPR: &str = "max_over_time(CONSUMER_FEE_P90[10m])>on()param_change_active{key=\"ConsumerFeeComfortP90Microunits\"}";
const EXPECTED_DEFERRAL_EXPR: &str = "(increase(INDUSTRIAL_DEFERRED_TOTAL[10m])/clamp_min(increase(INDUSTRIAL_ADMITTED_TOTAL[10m])+increase(INDUSTRIAL_DEFERRED_TOTAL[10m]),1))>0.3";
const EXPECTED_SUBSIDY_EXPR: &str = "increase(subsidy_cpu_ms_total[10m])>1e9orincrease(subsidy_bytes_total[10m])>1e12";
const EXPECTED_RENT_EXPR: &str = "increase(rent_escrow_locked_ct_total[10m])>1e6";
const EXPECTED_TLS_BURST_EXPR: &str = "sumby(prefix,code)(increase(tls_env_warning_total[5m]))>0";
const EXPECTED_TLS_NEW_DETAIL_EXPR: &str = "increase(maxby(prefix,code)(tls_env_warning_detail_unique_fingerprints)[10m])>0";
const EXPECTED_TLS_NEW_VARIABLES_EXPR: &str = "increase(maxby(prefix,code)(tls_env_warning_variables_unique_fingerprints)[10m])>0";
const EXPECTED_TLS_DETAIL_FLOOD_EXPR: &str = "sumby(prefix,code,fingerprint)(increase(tls_env_warning_detail_fingerprint_total{fingerprint!=\"none\"}[5m]))>10";
const EXPECTED_TLS_VARIABLES_FLOOD_EXPR: &str = "sumby(prefix,code,fingerprint)(increase(tls_env_warning_variables_fingerprint_total{fingerprint!=\"none\"}[5m]))>10";
const EXPECTED_TLS_STALE_EXPR: &str = "tls_env_warning_stale_snapshots>0";

const EXPECTED_DEP_DRIFT_EXPR: &str = "max(dependency_registry_check_status{status=\"drift\"})>0";
const EXPECTED_DEP_VIOLATIONS_EXPR: &str = "max(dependency_registry_check_status{status=\"violations\"})>0";
const EXPECTED_DEP_BASELINE_EXPR: &str = "max(dependency_registry_check_status{status=\"baseline_error\"})>0";
const EXPECTED_DEP_VIOLATION_SPIKE_EXPR: &str = "increase(dependency_policy_violation_total[5m])>0";

const EXPECTED_TREASURY_SNAPSHOT_EXPR: &str = "treasury_disbursement_snapshot_age_seconds>900";
const EXPECTED_TREASURY_OVERDUE_EXPR: &str = "treasury_disbursement_scheduled_oldest_age_seconds>7200";

const METRIC_DELTA: &str = "bridge_metric_delta";
const METRIC_RATE: &str = "bridge_metric_rate_per_second";

const EXPECTED_DELTA_SERIES: &[&str] = &["global-delta"];
const EXPECTED_RATE_SERIES: &[&str] = &["global-rate"];
const EXPECTED_DELTA_LABEL_SERIES: &[&str] = &["label-delta"];
const EXPECTED_RATE_LABEL_SERIES: &[&str] = &["label-rate"];

const EXPECTED_CONVERGENCE_SERIES: &[&str] = &["lag-critical"];
const EXPECTED_CONSUMER_FEE_SERIES: &[&str] = &["fee-comfort-breach"];
const EXPECTED_DEFERRAL_SERIES: &[&str] = &["deferral-high"];
const EXPECTED_SUBSIDY_SERIES: &[&str] = &["subsidy-cpu", "subsidy-bytes"];
const EXPECTED_RENT_SERIES: &[&str] = &["rent-surge"];
const EXPECTED_TLS_BURST_SERIES: &[&str] = &["tls-burst-main"];
const EXPECTED_TLS_NEW_DETAIL_SERIES: &[&str] = &["tls-new-detail"];
const EXPECTED_TLS_NEW_VARIABLES_SERIES: &[&str] = &["tls-new-vars"];
const EXPECTED_TLS_DETAIL_FLOOD_SERIES: &[&str] = &["tls-detail-flood"];
const EXPECTED_TLS_VARIABLES_FLOOD_SERIES: &[&str] = &["tls-vars-flood"];
const EXPECTED_TLS_STALE_SERIES: &[&str] = &["tls-stale"];

const EXPECTED_DEP_DRIFT_SERIES: &[&str] = &["registry-drift"];
const EXPECTED_DEP_VIOLATIONS_SERIES: &[&str] = &["registry-violation"];
const EXPECTED_DEP_BASELINE_SERIES: &[&str] = &["registry-baseline-error"];
const EXPECTED_DEP_VIOLATION_SPIKE_SERIES: &[&str] = &["policy-violation-spike"];

const EXPECTED_TREASURY_SNAPSHOT_SERIES: &[&str] = &["snapshot-stale"];
const EXPECTED_TREASURY_OVERDUE_SERIES: &[&str] = &["schedule-overdue"];

#[derive(Debug)]
pub enum ValidationError {
    Io(std::io::Error),
    MissingAlert(&'static str),
    ExpressionMismatch {
        alert: &'static str,
        expected: &'static str,
        actual: String,
    },
    UnexpectedAlertResult {
        alert: &'static str,
        expected: BTreeSet<String>,
        actual: BTreeSet<String>,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::Io(err) => write!(f, "failed to read alert rules: {err}"),
            ValidationError::MissingAlert(name) => {
                write!(f, "alert '{name}' missing from expected group")
            }
            ValidationError::ExpressionMismatch {
                alert,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "alert '{alert}' expression mismatch. expected '{expected}', found '{actual}'"
                )
            }
            ValidationError::UnexpectedAlertResult {
                alert,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "alert '{alert}' triggered {actual:?}, expected {expected:?}"
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ValidationError::Io(err) => Some(err),
            _ => None,
        }
    }
}

pub fn validate_bridge_alerts() -> Result<(), ValidationError> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(ALERT_FILE);
    let content = std::fs::read_to_string(&path).map_err(ValidationError::Io)?;
    validate_bridge_alerts_from_str(&content)
}

pub fn validate_chain_health_alerts() -> Result<(), ValidationError> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(ALERT_FILE);
    let content = std::fs::read_to_string(&path).map_err(ValidationError::Io)?;
    validate_chain_health_alerts_from_str(&content)
}

pub fn validate_dependency_registry_alerts() -> Result<(), ValidationError> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(ALERT_FILE);
    let content = std::fs::read_to_string(&path).map_err(ValidationError::Io)?;
    validate_dependency_registry_alerts_from_str(&content)
}

pub fn validate_treasury_alerts() -> Result<(), ValidationError> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(ALERT_FILE);
    let content = std::fs::read_to_string(&path).map_err(ValidationError::Io)?;
    validate_treasury_alerts_from_str(&content)
}

pub fn validate_all_alerts() -> Result<(), ValidationError> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(ALERT_FILE);
    let content = std::fs::read_to_string(&path).map_err(ValidationError::Io)?;
    validate_bridge_alerts_from_str(&content)?;
    validate_chain_health_alerts_from_str(&content)?;
    validate_dependency_registry_alerts_from_str(&content)?;
    validate_treasury_alerts_from_str(&content)
}

fn validate_bridge_alerts_from_str(content: &str) -> Result<(), ValidationError> {
    let bridge_alerts = extract_group_alerts(content, GROUP_BRIDGE);

    validate_expression(ALERT_DELTA, EXPECTED_DELTA_EXPR, &bridge_alerts)?;
    validate_expression(ALERT_RATE, EXPECTED_RATE_EXPR, &bridge_alerts)?;
    validate_expression(ALERT_DELTA_LABEL, EXPECTED_DELTA_LABEL_EXPR, &bridge_alerts)?;
    validate_expression(ALERT_RATE_LABEL, EXPECTED_RATE_LABEL_EXPR, &bridge_alerts)?;

    let dataset = build_bridge_dataset();

    validate_results(
        ALERT_DELTA,
        EXPECTED_DELTA_SERIES,
        evaluate_delta(&dataset, false, 3.0, 10.0, 6),
    )?;
    validate_results(
        ALERT_RATE,
        EXPECTED_RATE_SERIES,
        evaluate_rate(&dataset, false, 3.0, 0.5, 6),
    )?;
    validate_results(
        ALERT_DELTA_LABEL,
        EXPECTED_DELTA_LABEL_SERIES,
        evaluate_delta(&dataset, true, 3.0, 5.0, 6),
    )?;
    validate_results(
        ALERT_RATE_LABEL,
        EXPECTED_RATE_LABEL_SERIES,
        evaluate_rate(&dataset, true, 3.0, 0.25, 6),
    )?;

    Ok(())
}

fn validate_chain_health_alerts_from_str(content: &str) -> Result<(), ValidationError> {
    let alerts = extract_group_alerts(content, GROUP_CHAIN_HEALTH);

    validate_expression(ALERT_CONVERGENCE_LAG, EXPECTED_CONVERGENCE_EXPR, &alerts)?;
    validate_expression(ALERT_CONSUMER_FEE, EXPECTED_CONSUMER_FEE_EXPR, &alerts)?;
    validate_expression(ALERT_DEFERRAL_RATIO, EXPECTED_DEFERRAL_EXPR, &alerts)?;
    validate_expression(ALERT_SUBSIDY_SPIKE, EXPECTED_SUBSIDY_EXPR, &alerts)?;
    validate_expression(ALERT_RENT_ESCROW, EXPECTED_RENT_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_BURST, EXPECTED_TLS_BURST_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_NEW_DETAIL, EXPECTED_TLS_NEW_DETAIL_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_NEW_VARIABLES, EXPECTED_TLS_NEW_VARIABLES_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_DETAIL_FLOOD, EXPECTED_TLS_DETAIL_FLOOD_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_VARIABLES_FLOOD, EXPECTED_TLS_VARIABLES_FLOOD_EXPR, &alerts)?;
    validate_expression(ALERT_TLS_STALE, EXPECTED_TLS_STALE_EXPR, &alerts)?;

    let dataset = build_chain_health_dataset();

    validate_results(
        ALERT_CONVERGENCE_LAG,
        EXPECTED_CONVERGENCE_SERIES,
        evaluate_convergence(&dataset.convergence, 30.0),
    )?;
    validate_results(
        ALERT_CONSUMER_FEE,
        EXPECTED_CONSUMER_FEE_SERIES,
        evaluate_consumer_fee(&dataset.consumer_fee),
    )?;
    validate_results(
        ALERT_DEFERRAL_RATIO,
        EXPECTED_DEFERRAL_SERIES,
        evaluate_deferral_ratio(&dataset.deferral_ratio, 0.3),
    )?;
    validate_results(
        ALERT_SUBSIDY_SPIKE,
        EXPECTED_SUBSIDY_SERIES,
        evaluate_subsidy_spike(&dataset.subsidy_growth, 1.0e9, 1.0e12),
    )?;
    validate_results(
        ALERT_RENT_ESCROW,
        EXPECTED_RENT_SERIES,
        evaluate_rent_spike(&dataset.rent_escrow, 1.0e6),
    )?;
    validate_results(
        ALERT_TLS_BURST,
        EXPECTED_TLS_BURST_SERIES,
        evaluate_tls_burst(&dataset.tls_bursts),
    )?;
    validate_results(
        ALERT_TLS_NEW_DETAIL,
        EXPECTED_TLS_NEW_DETAIL_SERIES,
        evaluate_tls_new_fingerprint(&dataset.tls_new_detail),
    )?;
    validate_results(
        ALERT_TLS_NEW_VARIABLES,
        EXPECTED_TLS_NEW_VARIABLES_SERIES,
        evaluate_tls_new_fingerprint(&dataset.tls_new_variables),
    )?;
    validate_results(
        ALERT_TLS_DETAIL_FLOOD,
        EXPECTED_TLS_DETAIL_FLOOD_SERIES,
        evaluate_tls_fingerprint_flood(&dataset.tls_detail_flood, 10),
    )?;
    validate_results(
        ALERT_TLS_VARIABLES_FLOOD,
        EXPECTED_TLS_VARIABLES_FLOOD_SERIES,
        evaluate_tls_fingerprint_flood(&dataset.tls_variables_flood, 10),
    )?;
    validate_results(
        ALERT_TLS_STALE,
        EXPECTED_TLS_STALE_SERIES,
        evaluate_tls_stale(&dataset.tls_stale),
    )?;

    Ok(())
}

fn validate_dependency_registry_alerts_from_str(content: &str) -> Result<(), ValidationError> {
    let alerts = extract_group_alerts(content, GROUP_DEPENDENCY_REGISTRY);

    validate_expression(ALERT_DEP_DRIFT, EXPECTED_DEP_DRIFT_EXPR, &alerts)?;
    validate_expression(ALERT_DEP_VIOLATIONS, EXPECTED_DEP_VIOLATIONS_EXPR, &alerts)?;
    validate_expression(ALERT_DEP_BASELINE, EXPECTED_DEP_BASELINE_EXPR, &alerts)?;
    validate_expression(
        ALERT_DEP_VIOLATION_SPIKE,
        EXPECTED_DEP_VIOLATION_SPIKE_EXPR,
        &alerts,
    )?;

    let dataset = build_dependency_dataset();

    validate_results(
        ALERT_DEP_DRIFT,
        EXPECTED_DEP_DRIFT_SERIES,
        evaluate_dependency_status(&dataset.statuses, "drift"),
    )?;
    validate_results(
        ALERT_DEP_VIOLATIONS,
        EXPECTED_DEP_VIOLATIONS_SERIES,
        evaluate_dependency_status(&dataset.statuses, "violations"),
    )?;
    validate_results(
        ALERT_DEP_BASELINE,
        EXPECTED_DEP_BASELINE_SERIES,
        evaluate_dependency_status(&dataset.statuses, "baseline_error"),
    )?;
    validate_results(
        ALERT_DEP_VIOLATION_SPIKE,
        EXPECTED_DEP_VIOLATION_SPIKE_SERIES,
        evaluate_dependency_violation(&dataset.policy_violations),
    )?;

    Ok(())
}

fn validate_treasury_alerts_from_str(content: &str) -> Result<(), ValidationError> {
    let alerts = extract_group_alerts(content, GROUP_TREASURY);

    validate_expression(ALERT_TREASURY_SNAPSHOT, EXPECTED_TREASURY_SNAPSHOT_EXPR, &alerts)?;
    validate_expression(ALERT_TREASURY_OVERDUE, EXPECTED_TREASURY_OVERDUE_EXPR, &alerts)?;

    let dataset = build_treasury_dataset();

    validate_results(
        ALERT_TREASURY_SNAPSHOT,
        EXPECTED_TREASURY_SNAPSHOT_SERIES,
        evaluate_treasury_threshold(&dataset.snapshot_age, 900.0),
    )?;
    validate_results(
        ALERT_TREASURY_OVERDUE,
        EXPECTED_TREASURY_OVERDUE_SERIES,
        evaluate_treasury_threshold(&dataset.schedule_age, 7200.0),
    )?;

    Ok(())
}

fn validate_expression(
    alert: &'static str,
    expected: &'static str,
    alerts: &BTreeMap<String, String>,
) -> Result<(), ValidationError> {
    let expression = alerts
        .get(alert)
        .ok_or(ValidationError::MissingAlert(alert))?;
    let actual = normalize_expression(expression);
    if actual != expected {
        return Err(ValidationError::ExpressionMismatch {
            alert,
            expected,
            actual,
        });
    }
    Ok(())
}

fn validate_results(
    alert: &'static str,
    expected: &[&str],
    actual: BTreeSet<String>,
) -> Result<(), ValidationError> {
    let expected_set: BTreeSet<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if actual != expected_set {
        return Err(ValidationError::UnexpectedAlertResult {
            alert,
            expected: expected_set,
            actual,
        });
    }
    Ok(())
}

fn evaluate_delta(
    dataset: &[Series],
    require_labels: bool,
    multiplier: f64,
    min_absolute: f64,
    min_samples: usize,
) -> BTreeSet<String> {
    dataset
        .iter()
        .filter(|series| series.metric == METRIC_DELTA)
        .filter(|series| !require_labels || !series.labels.is_empty())
        .filter(|series| series.values.len() >= min_samples)
        .filter(|series| {
            if let (Some(current), Some(average)) = (series.current(), series.average()) {
                current > average * multiplier && current > min_absolute
            } else {
                false
            }
        })
        .map(|series| series.name.to_string())
        .collect()
}

fn evaluate_rate(
    dataset: &[Series],
    require_labels: bool,
    multiplier: f64,
    min_rate: f64,
    min_samples: usize,
) -> BTreeSet<String> {
    dataset
        .iter()
        .filter(|series| series.metric == METRIC_RATE)
        .filter(|series| !require_labels || !series.labels.is_empty())
        .filter(|series| series.values.len() >= min_samples)
        .filter(|series| {
            if let (Some(current), Some(average)) = (series.current(), series.average()) {
                current > average * multiplier && current > min_rate
            } else {
                false
            }
        })
        .map(|series| series.name.to_string())
        .collect()
}

fn evaluate_convergence(samples: &[ConvergenceSample], threshold: f64) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.quantile > threshold)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_consumer_fee(samples: &[ConsumerFeeSample]) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.p90 > sample.comfort)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_deferral_ratio(samples: &[DeferralSample], threshold: f64) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| {
            let admitted = sample.admitted.max(0.0);
            let deferred = sample.deferred.max(0.0);
            let total = admitted + deferred;
            if total <= 0.0 {
                return false;
            }
            let ratio = deferred / total.max(1.0);
            ratio > threshold
        })
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_subsidy_spike(
    samples: &[SubsidySample],
    cpu_threshold: f64,
    bytes_threshold: f64,
) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| {
            sample.cpu_increase > cpu_threshold || sample.bytes_increase > bytes_threshold
        })
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_rent_spike(samples: &[RentSample], threshold: f64) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.increase > threshold)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_tls_burst(samples: &[TlsBurstSample]) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.delta > 0)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_tls_new_fingerprint(samples: &[TlsNewFingerprintSample]) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.new_seen)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_tls_fingerprint_flood(
    samples: &[TlsFloodSample],
    threshold: u64,
) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.delta > threshold)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_tls_stale(samples: &[TlsStaleSample]) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.stale > 0)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_dependency_status(
    samples: &[DependencyStatusSample],
    status: &str,
) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.status == status && sample.value > 0.0)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_dependency_violation(samples: &[DependencyViolationSample]) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.delta > 0.0)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn evaluate_treasury_threshold(
    samples: &[TreasurySample],
    threshold: f64,
) -> BTreeSet<String> {
    samples
        .iter()
        .filter(|sample| sample.age_secs > threshold)
        .map(|sample| sample.name.to_string())
        .collect()
}

fn extract_group_alerts(content: &str, group: &str) -> BTreeMap<String, String> {
    let mut alerts = BTreeMap::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut index = 0usize;
    let mut in_group = false;
    let mut current_alert: Option<String> = None;
    let mut collecting_expr = false;
    let mut expr_indent = 0usize;
    let mut expr_buffer = String::new();

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if collecting_expr {
            if trimmed.is_empty() {
                expr_buffer.push('\n');
                index += 1;
                continue;
            }
            if indent < expr_indent {
                if let Some(alert) = current_alert.clone() {
                    if !expr_buffer.is_empty() {
                        alerts.insert(alert, expr_buffer.trim_end().to_string());
                    }
                }
                expr_buffer.clear();
                collecting_expr = false;
                continue;
            }
            let slice = if line.len() >= expr_indent {
                &line[expr_indent..]
            } else {
                trimmed
            };
            expr_buffer.push_str(slice);
            expr_buffer.push('\n');
            index += 1;
            continue;
        }

        if indent == 2 && trimmed.starts_with("- name: ") {
            in_group = trimmed[8..].trim() == group;
            if !in_group {
                current_alert = None;
            }
            index += 1;
            continue;
        }

        if !in_group {
            index += 1;
            continue;
        }

        if indent == 6 && trimmed.starts_with("- alert: ") {
            if let Some(alert) = current_alert.take() {
                if !expr_buffer.is_empty() {
                    alerts.insert(alert, expr_buffer.trim_end().to_string());
                    expr_buffer.clear();
                }
            }
            current_alert = Some(trimmed[9..].trim().to_string());
            index += 1;
            continue;
        }

        if let Some(_) = current_alert {
            if trimmed.starts_with("expr:") {
                if trimmed == "expr: |" {
                    collecting_expr = true;
                    expr_indent = indent + 2;
                    expr_buffer.clear();
                    index += 1;
                    continue;
                } else {
                    let value = trimmed[5..].trim_start();
                    if let Some(alert) = current_alert.take() {
                        alerts.insert(alert, value.to_string());
                    }
                    index += 1;
                    continue;
                }
            }
        }

        index += 1;
    }

    if collecting_expr {
        if let Some(alert) = current_alert {
            if !expr_buffer.is_empty() {
                alerts.insert(alert, expr_buffer.trim_end().to_string());
            }
        }
    } else if let Some(alert) = current_alert {
        if !expr_buffer.is_empty() {
            alerts.insert(alert, expr_buffer.trim_end().to_string());
        }
    }

    alerts
}

fn normalize_expression(expr: &str) -> String {
    expr.split_whitespace().collect()
}

fn build_bridge_dataset() -> Vec<Series> {
    vec![
        Series::new(
            "global-delta",
            METRIC_DELTA,
            "",
            build_series(2.0, 24, 40.0, 7),
        ),
        Series::new(
            "global-rate",
            METRIC_RATE,
            "",
            build_series(0.1, 24, 1.2, 7),
        ),
        Series::new(
            "label-delta",
            METRIC_DELTA,
            "asset=btc",
            build_series(0.5, 24, 9.0, 7),
        ),
        Series::new(
            "label-rate",
            METRIC_RATE,
            "asset=btc",
            build_series(0.05, 24, 0.4, 7),
        ),
        Series::new(
            "quiet-delta",
            METRIC_DELTA,
            "asset=eth",
            build_uniform(0.5, 31),
        ),
        Series::new(
            "quiet-rate",
            METRIC_RATE,
            "asset=eth",
            build_uniform(0.05, 31),
        ),
        Series::new(
            "recovery-delta",
            METRIC_DELTA,
            "asset=dot",
            build_recovery_series(0.6, 20, 7.5, &[3.0, 1.4, 0.7, 0.6]),
        ),
        Series::new(
            "recovery-rate",
            METRIC_RATE,
            "asset=dot",
            build_recovery_series(0.04, 20, 0.35, &[0.18, 0.07, 0.05, 0.04]),
        ),
        Series::new(
            "recovery-label-delta",
            METRIC_DELTA,
            "asset=sol",
            build_recovery_series(0.8, 18, 8.0, &[2.6, 1.1, 0.85, 0.8]),
        ),
        Series::new(
            "recovery-label-rate",
            METRIC_RATE,
            "asset=sol",
            build_recovery_series(0.06, 18, 0.45, &[0.21, 0.08, 0.06, 0.05]),
        ),
        Series::new(
            "partial-window-delta",
            METRIC_DELTA,
            "asset=ltc",
            vec![1.0, 1.2, 1.15, 1.1],
        ),
        Series::new(
            "partial-window-rate",
            METRIC_RATE,
            "asset=ltc",
            vec![0.08, 0.1, 0.12, 0.11],
        ),
        Series::new(
            "partial-window-dispute-delta",
            METRIC_DELTA,
            "kind=challenge,outcome=penalized",
            vec![0.2, 0.24, 0.23, 0.22],
        ),
        Series::new(
            "partial-window-dispute-rate",
            METRIC_RATE,
            "kind=challenge,outcome=penalized",
            vec![0.015, 0.02, 0.019, 0.018],
        ),
        Series::new(
            "recovery-approvals-delta",
            METRIC_DELTA,
            "result=failed,reason=quorum", 
            build_recovery_series(0.4, 18, 3.8, &[1.4, 0.7, 0.5, 0.4]),
        ),
        Series::new(
            "recovery-approvals-rate",
            METRIC_RATE,
            "result=failed,reason=quorum",
            build_recovery_series(0.03, 18, 0.24, &[0.11, 0.05, 0.04, 0.03]),
        ),
    ]
}

fn build_chain_health_dataset() -> ChainHealthDataset {
    ChainHealthDataset {
        convergence: vec![
            ConvergenceSample {
                name: "lag-critical",
                quantile: 44.0,
            },
            ConvergenceSample {
                name: "lag-healthy",
                quantile: 18.0,
            },
        ],
        consumer_fee: vec![
            ConsumerFeeSample {
                name: "fee-comfort-breach",
                p90: 950_000.0,
                comfort: 900_000.0,
            },
            ConsumerFeeSample {
                name: "fee-within-band",
                p90: 700_000.0,
                comfort: 900_000.0,
            },
        ],
        deferral_ratio: vec![
            DeferralSample {
                name: "deferral-high",
                admitted: 80.0,
                deferred: 40.0,
            },
            DeferralSample {
                name: "deferral-healthy",
                admitted: 140.0,
                deferred: 10.0,
            },
        ],
        subsidy_growth: vec![
            SubsidySample {
                name: "subsidy-cpu",
                cpu_increase: 1.2e9,
                bytes_increase: 5.0e11,
            },
            SubsidySample {
                name: "subsidy-bytes",
                cpu_increase: 2.0e8,
                bytes_increase: 1.5e12,
            },
            SubsidySample {
                name: "subsidy-normal",
                cpu_increase: 1.0e8,
                bytes_increase: 2.0e11,
            },
        ],
        rent_escrow: vec![
            RentSample {
                name: "rent-surge",
                increase: 1.5e6,
            },
            RentSample {
                name: "rent-normal",
                increase: 5.0e5,
            },
        ],
        tls_bursts: vec![
            TlsBurstSample {
                name: "tls-burst-main",
                delta: 3,
            },
            TlsBurstSample {
                name: "tls-burst-quiet",
                delta: 0,
            },
        ],
        tls_new_detail: vec![
            TlsNewFingerprintSample {
                name: "tls-new-detail",
                new_seen: true,
            },
            TlsNewFingerprintSample {
                name: "tls-detail-repeat",
                new_seen: false,
            },
        ],
        tls_new_variables: vec![
            TlsNewFingerprintSample {
                name: "tls-new-vars",
                new_seen: true,
            },
            TlsNewFingerprintSample {
                name: "tls-vars-repeat",
                new_seen: false,
            },
        ],
        tls_detail_flood: vec![
            TlsFloodSample {
                name: "tls-detail-flood",
                delta: 12,
            },
            TlsFloodSample {
                name: "tls-detail-calm",
                delta: 1,
            },
        ],
        tls_variables_flood: vec![
            TlsFloodSample {
                name: "tls-vars-flood",
                delta: 15,
            },
            TlsFloodSample {
                name: "tls-vars-calm",
                delta: 2,
            },
        ],
        tls_stale: vec![
            TlsStaleSample {
                name: "tls-stale",
                stale: 2,
            },
            TlsStaleSample {
                name: "tls-fresh",
                stale: 0,
            },
        ],
    }
}

fn build_dependency_dataset() -> DependencyDataset {
    DependencyDataset {
        statuses: vec![
            DependencyStatusSample {
                name: "registry-drift",
                status: "drift",
                value: 1.0,
            },
            DependencyStatusSample {
                name: "registry-healthy",
                status: "drift",
                value: 0.0,
            },
            DependencyStatusSample {
                name: "registry-violation",
                status: "violations",
                value: 1.0,
            },
            DependencyStatusSample {
                name: "registry-clean",
                status: "violations",
                value: 0.0,
            },
            DependencyStatusSample {
                name: "registry-baseline-error",
                status: "baseline_error",
                value: 1.0,
            },
        ],
        policy_violations: vec![
            DependencyViolationSample {
                name: "policy-violation-spike",
                delta: 3.0,
            },
            DependencyViolationSample {
                name: "policy-stable",
                delta: 0.0,
            },
        ],
    }
}

fn build_treasury_dataset() -> TreasuryDataset {
    TreasuryDataset {
        snapshot_age: vec![
            TreasurySample {
                name: "snapshot-stale",
                age_secs: 1800.0,
            },
            TreasurySample {
                name: "snapshot-fresh",
                age_secs: 300.0,
            },
        ],
        schedule_age: vec![
            TreasurySample {
                name: "schedule-overdue",
                age_secs: 8200.0,
            },
            TreasurySample {
                name: "schedule-on-track",
                age_secs: 5400.0,
            },
        ],
    }
}

fn build_series(base: f64, base_len: usize, spike: f64, spike_len: usize) -> Vec<f64> {
    let mut values = Vec::with_capacity(base_len + spike_len);
    values.extend(std::iter::repeat(base).take(base_len));
    values.extend(std::iter::repeat(spike).take(spike_len));
    values
}

fn build_uniform(value: f64, len: usize) -> Vec<f64> {
    std::iter::repeat(value).take(len).collect()
}

fn build_recovery_series(base: f64, base_len: usize, spike: f64, tail: &[f64]) -> Vec<f64> {
    let mut values = Vec::with_capacity(base_len + 1 + tail.len());
    values.extend(std::iter::repeat(base).take(base_len));
    values.push(spike);
    values.extend(tail.iter().copied());
    values
}

#[derive(Clone)]
struct Series {
    name: &'static str,
    metric: &'static str,
    labels: &'static str,
    values: Vec<f64>,
}

impl Series {
    fn new(name: &'static str, metric: &'static str, labels: &'static str, values: Vec<f64>) -> Self {
        Self {
            name,
            metric,
            labels,
            values,
        }
    }

    fn current(&self) -> Option<f64> {
        self.values.last().copied()
    }

    fn average(&self) -> Option<f64> {
        if self.values.is_empty() {
            None
        } else {
            Some(self.values.iter().copied().sum::<f64>() / self.values.len() as f64)
        }
    }
}

struct ChainHealthDataset {
    convergence: Vec<ConvergenceSample>,
    consumer_fee: Vec<ConsumerFeeSample>,
    deferral_ratio: Vec<DeferralSample>,
    subsidy_growth: Vec<SubsidySample>,
    rent_escrow: Vec<RentSample>,
    tls_bursts: Vec<TlsBurstSample>,
    tls_new_detail: Vec<TlsNewFingerprintSample>,
    tls_new_variables: Vec<TlsNewFingerprintSample>,
    tls_detail_flood: Vec<TlsFloodSample>,
    tls_variables_flood: Vec<TlsFloodSample>,
    tls_stale: Vec<TlsStaleSample>,
}

struct ConvergenceSample {
    name: &'static str,
    quantile: f64,
}

struct ConsumerFeeSample {
    name: &'static str,
    p90: f64,
    comfort: f64,
}

struct DeferralSample {
    name: &'static str,
    admitted: f64,
    deferred: f64,
}

struct SubsidySample {
    name: &'static str,
    cpu_increase: f64,
    bytes_increase: f64,
}

struct RentSample {
    name: &'static str,
    increase: f64,
}

struct TlsBurstSample {
    name: &'static str,
    delta: u64,
}

struct TlsNewFingerprintSample {
    name: &'static str,
    new_seen: bool,
}

struct TlsFloodSample {
    name: &'static str,
    delta: u64,
}

struct TlsStaleSample {
    name: &'static str,
    stale: u64,
}

struct DependencyDataset {
    statuses: Vec<DependencyStatusSample>,
    policy_violations: Vec<DependencyViolationSample>,
}

struct DependencyStatusSample {
    name: &'static str,
    status: &'static str,
    value: f64,
}

struct DependencyViolationSample {
    name: &'static str,
    delta: f64,
}

struct TreasuryDataset {
    snapshot_age: Vec<TreasurySample>,
    schedule_age: Vec<TreasurySample>,
}

struct TreasurySample {
    name: &'static str,
    age_secs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bridge_alerts() {
        let content = include_str!("../alert.rules.yml");
        let alerts = extract_group_alerts(content, GROUP_BRIDGE);
        assert!(alerts.contains_key(ALERT_DELTA));
        assert!(alerts.contains_key(ALERT_RATE));
        assert!(alerts.contains_key(ALERT_DELTA_LABEL));
        assert!(alerts.contains_key(ALERT_RATE_LABEL));
    }

    #[test]
    fn validates_bridge_rules() {
        validate_bridge_alerts().expect("bridge alerts valid");
    }

    #[test]
    fn validates_chain_health_rules() {
        validate_chain_health_alerts().expect("chain health alerts valid");
    }

    #[test]
    fn validates_dependency_rules() {
        validate_dependency_registry_alerts().expect("dependency alerts valid");
    }

    #[test]
    fn validates_treasury_rules() {
        validate_treasury_alerts().expect("treasury alerts valid");
    }

    #[test]
    fn validates_all_rules() {
        validate_all_alerts().expect("all alerts valid");
    }
}
