use crate::{http_client, parse_utils::parse_usize};
use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use diagnostics::anyhow::{self, Context, Result as AnyhowResult};
use foundation_serialization::json::{self, Value as JsonValue};
use foundation_time::UtcDateTime;
use httpd::Method;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

pub enum RemediationCmd {
    Bridge {
        aggregator: String,
        limit: usize,
        dispatch_limit: usize,
    },
}

impl RemediationCmd {
    pub fn command() -> Command {
        CommandBuilder::new(
            CommandId("remediation"),
            "remediation",
            "Remediation dashboards and runbooks",
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("remediation.bridge"),
                "bridge",
                "Inspect bridge remediation actions",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("aggregator", "aggregator", "Metrics aggregator base URL")
                    .default("http://localhost:9000"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("limit", "limit", "Maximum remediation actions to display")
                    .default("5"),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "dispatch_limit",
                    "dispatch-limit",
                    "Maximum dispatch records per action",
                )
                .default("5"),
            ))
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'remediation'".to_string())?;

        match name {
            "bridge" => {
                let aggregator = sub_matches
                    .get_string("aggregator")
                    .unwrap_or_else(|| "http://localhost:9000".to_string());
                let limit = parse_usize(sub_matches.get_string("limit"), "limit")?
                    .unwrap_or(5)
                    .max(1);
                let dispatch_limit =
                    parse_usize(sub_matches.get_string("dispatch_limit"), "dispatch-limit")?
                        .unwrap_or(5)
                        .max(1);
                Ok(RemediationCmd::Bridge {
                    aggregator,
                    limit,
                    dispatch_limit,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'remediation'")),
        }
    }
}

pub fn handle(cmd: RemediationCmd) {
    match cmd {
        RemediationCmd::Bridge {
            aggregator,
            limit,
            dispatch_limit,
        } => match render_bridge_remediation(&aggregator, limit, dispatch_limit) {
            Ok(report) => print!("{report}"),
            Err(err) => eprintln!("remediation bridge failed: {err}"),
        },
    }
}

fn render_bridge_remediation(
    aggregator: &str,
    limit: usize,
    dispatch_limit: usize,
) -> AnyhowResult<String> {
    let actions =
        fetch_bridge_actions(aggregator).context("failed to fetch bridge remediation actions")?;
    let dispatches = fetch_bridge_dispatches(aggregator)
        .context("failed to fetch bridge remediation dispatch log")?;
    Ok(render_bridge_report(
        actions,
        dispatches,
        limit,
        dispatch_limit,
    ))
}

fn fetch_bridge_actions(base: &str) -> AnyhowResult<Vec<RemediationAction>> {
    let client = http_client::blocking_client();
    let url = format!("{}/remediation/bridge", base.trim_end_matches('/'));
    let response = client
        .request(Method::Get, &url)
        .map_err(anyhow::Error::from_error)?
        .send()
        .map_err(anyhow::Error::from_error)?;
    if !response.status().is_success() {
        anyhow::bail!(
            "aggregator responded with status {}",
            response.status().as_u16()
        );
    }
    let body = response.into_body();
    let value: JsonValue = json::from_slice(&body).map_err(anyhow::Error::from_error)?;
    parse_remediation_actions(value)
}

fn fetch_bridge_dispatches(base: &str) -> AnyhowResult<Vec<RemediationDispatch>> {
    let client = http_client::blocking_client();
    let url = format!(
        "{}/remediation/bridge/dispatches",
        base.trim_end_matches('/')
    );
    let response = client
        .request(Method::Get, &url)
        .map_err(anyhow::Error::from_error)?
        .send()
        .map_err(anyhow::Error::from_error)?;
    if !response.status().is_success() {
        anyhow::bail!(
            "aggregator responded with status {}",
            response.status().as_u16()
        );
    }
    let body = response.into_body();
    let value: JsonValue = json::from_slice(&body).map_err(anyhow::Error::from_error)?;
    parse_remediation_dispatches(value)
}

fn parse_remediation_actions(value: JsonValue) -> AnyhowResult<Vec<RemediationAction>> {
    let array = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("remediation actions payload must be an array"))?;
    let mut actions = Vec::with_capacity(array.len());
    for entry in array {
        let object = entry
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("remediation action must be a JSON object"))?;
        let action = RemediationAction {
            action: required_string(object, "action", "remediation action")?,
            playbook: required_string(object, "playbook", "remediation action")?,
            peer_id: required_string(object, "peer_id", "remediation action")?,
            metric: required_string(object, "metric", "remediation action")?,
            labels: parse_labels(object.get("labels")),
            occurrences: object
                .get("occurrences")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
            delta: object
                .get("delta")
                .and_then(JsonValue::as_f64)
                .unwrap_or(0.0),
            ratio: object
                .get("ratio")
                .and_then(JsonValue::as_f64)
                .unwrap_or(0.0),
            timestamp: required_u64(object, "timestamp", "remediation action")?,
            acknowledged_at: object.get("acknowledged_at").and_then(JsonValue::as_u64),
            closed_out_at: object.get("closed_out_at").and_then(JsonValue::as_u64),
            acknowledgement_notes: object
                .get("acknowledgement_notes")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            first_dispatch_at: object.get("first_dispatch_at").and_then(JsonValue::as_u64),
            last_dispatch_at: object.get("last_dispatch_at").and_then(JsonValue::as_u64),
            pending_since: object.get("pending_since").and_then(JsonValue::as_u64),
            pending_escalated: object
                .get("pending_escalated")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false),
            dispatch_attempts: object
                .get("dispatch_attempts")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
            auto_retry_count: object
                .get("auto_retry_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
            last_auto_retry_at: object.get("last_auto_retry_at").and_then(JsonValue::as_u64),
            last_ack_state: object
                .get("last_ack_state")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            last_ack_notes: object
                .get("last_ack_notes")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            follow_up_notes: object
                .get("follow_up_notes")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            response_sequence: parse_string_array(object.get("response_sequence")),
            dashboard_panels: parse_string_array(object.get("dashboard_panels")),
            runbook_path: object
                .get("runbook_path")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            dispatch_endpoint: object
                .get("dispatch_endpoint")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
        };
        actions.push(action);
    }
    Ok(actions)
}

fn parse_remediation_dispatches(value: JsonValue) -> AnyhowResult<Vec<RemediationDispatch>> {
    let array = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("dispatch log payload must be an array"))?;
    let mut dispatches = Vec::with_capacity(array.len());
    for entry in array {
        let object = entry
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("dispatch entry must be a JSON object"))?;
        let dispatch = RemediationDispatch {
            action_timestamp: required_u64(object, "timestamp", "dispatch record")?,
            action_kind: required_string(object, "action", "dispatch record")?,
            playbook: required_string(object, "playbook", "dispatch record")?,
            peer_id: required_string(object, "peer_id", "dispatch record")?,
            metric: required_string(object, "metric", "dispatch record")?,
            target: required_string(object, "target", "dispatch record")?,
            status: required_string(object, "status", "dispatch record")?,
            dispatched_at: required_u64(object, "dispatched_at", "dispatch record")?,
            acknowledgement: parse_dispatch_ack(object.get("acknowledgement"))?,
            annotation: object
                .get("annotation")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
        };
        dispatches.push(dispatch);
    }
    Ok(dispatches)
}

fn render_bridge_report(
    mut actions: Vec<RemediationAction>,
    dispatches: Vec<RemediationDispatch>,
    limit: usize,
    dispatch_limit: usize,
) -> String {
    actions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    for action in &mut actions {
        action.labels.sort_by(|a, b| a.key.cmp(&b.key));
    }

    let mut dispatch_map = group_dispatches(dispatches);
    let now = current_unix_timestamp();
    let mut output = String::new();

    if actions.is_empty() {
        output.push_str("no remediation actions recorded\n");
        let mut remaining: Vec<RemediationDispatch> = dispatch_map
            .into_values()
            .flat_map(|entries| entries.into_iter())
            .collect();
        if !remaining.is_empty() {
            remaining.sort_by(|a, b| b.dispatched_at.cmp(&a.dispatched_at));
            output.push_str("\nRecent dispatches:\n");
            for dispatch in remaining.into_iter().take(dispatch_limit) {
                render_dispatch_summary(&mut output, &dispatch, now)
                    .expect("write dispatch summary");
            }
        }
        return output;
    }

    let total_actions = actions.len();
    for (index, action) in actions.into_iter().take(limit).enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(
            output,
            "Action {}: {} · {}",
            index + 1,
            action.action,
            action.playbook
        )
        .expect("write action header");
        writeln!(
            output,
            "  Peer: {}  Metric: {}",
            action.peer_id, action.metric
        )
        .expect("write peer/metric");
        if !action.labels.is_empty() {
            let labels: Vec<String> = action
                .labels
                .iter()
                .map(|label| format!("{}={}", label.key, label.value))
                .collect();
            writeln!(output, "  Labels: {}", labels.join(", ")).expect("write labels");
        }
        writeln!(
            output,
            "  Triggered at: {}",
            format_timestamp_with_relative(Some(action.timestamp), now)
        )
        .expect("write trigger time");
        writeln!(
            output,
            "  Occurrences: {}  Delta: {:.2}  Ratio: {:.2}×",
            action.occurrences, action.delta, action.ratio
        )
        .expect("write statistics");
        writeln!(
            output,
            "  Dispatch attempts: {}  Auto retries: {}",
            action.dispatch_attempts, action.auto_retry_count
        )
        .expect("write attempts");
        writeln!(
            output,
            "  First dispatch at: {}",
            format_timestamp_with_relative(action.first_dispatch_at, now)
        )
        .expect("write first dispatch");
        writeln!(
            output,
            "  Last dispatch at: {}",
            format_timestamp_with_relative(action.last_dispatch_at, now)
        )
        .expect("write last dispatch");
        writeln!(
            output,
            "  Pending since: {}",
            format_timestamp_with_relative(action.pending_since, now)
        )
        .expect("write pending since");
        if action.pending_escalated {
            output.push_str("  Escalation queued: yes\n");
        }
        if let Some(last_retry) = action.last_auto_retry_at {
            writeln!(
                output,
                "  Last auto retry: {}",
                format_timestamp_with_relative(Some(last_retry), now)
            )
            .expect("write last auto retry");
        }
        if let Some(state) = action.last_ack_state.as_deref() {
            let notes = action.last_ack_notes.as_deref().unwrap_or("");
            writeln!(output, "  Last acknowledgement: {state} (notes: {notes})")
                .expect("write ack state");
        }
        if let Some(notes) = action.acknowledgement_notes.as_deref() {
            writeln!(output, "  Acknowledgement notes: {notes}").expect("write ack notes");
        }
        if let Some(ack) = action.acknowledged_at {
            let latency = action
                .first_dispatch_at
                .map(|first| ack.saturating_sub(first));
            if let Some(duration) = latency {
                writeln!(
                    output,
                    "  Acknowledged at: {} ({} after first dispatch)",
                    format_timestamp_with_relative(Some(ack), now),
                    format_duration(duration)
                )
                .expect("write ack timestamp");
            } else {
                writeln!(
                    output,
                    "  Acknowledged at: {}",
                    format_timestamp_with_relative(Some(ack), now)
                )
                .expect("write ack timestamp");
            }
        } else {
            output.push_str("  Acknowledged at: -\n");
        }
        if let Some(closed) = action.closed_out_at {
            let latency = action
                .first_dispatch_at
                .map(|first| closed.saturating_sub(first));
            if let Some(duration) = latency {
                writeln!(
                    output,
                    "  Closed out at: {} ({} after first dispatch)",
                    format_timestamp_with_relative(Some(closed), now),
                    format_duration(duration)
                )
                .expect("write closed timestamp");
            } else {
                writeln!(
                    output,
                    "  Closed out at: {}",
                    format_timestamp_with_relative(Some(closed), now)
                )
                .expect("write closed timestamp");
            }
        } else {
            output.push_str("  Closed out at: -\n");
        }
        if let Some(notes) = action.follow_up_notes.as_deref() {
            writeln!(output, "  Follow-ups: {notes}").expect("write follow-ups");
        }
        if let Some(path) = action.runbook_path.as_deref() {
            writeln!(output, "  Runbook: {path}").expect("write runbook");
        }
        if let Some(endpoint) = action.dispatch_endpoint.as_deref() {
            writeln!(output, "  Dispatch endpoint: {endpoint}").expect("write endpoint");
        }
        if !action.dashboard_panels.is_empty() {
            writeln!(
                output,
                "  Dashboard panels: {}",
                action.dashboard_panels.join(", ")
            )
            .expect("write panels");
        }
        if !action.response_sequence.is_empty() {
            output.push_str("  Response sequence:\n");
            for step in &action.response_sequence {
                writeln!(output, "    - {step}").expect("write sequence");
            }
        }

        let key = action.dispatch_key();
        let dispatch_entries = dispatch_map.remove(&key).unwrap_or_default();
        if dispatch_entries.is_empty() {
            output.push_str("  Dispatch history: none recorded\n");
        } else {
            output.push_str("  Dispatch history:\n");
            for dispatch in dispatch_entries.iter().take(dispatch_limit) {
                render_dispatch_summary(&mut output, dispatch, now)
                    .expect("write dispatch summary");
            }
            if dispatch_entries.len() > dispatch_limit {
                writeln!(
                    output,
                    "    … {} additional dispatches omitted",
                    dispatch_entries.len() - dispatch_limit
                )
                .expect("write dispatch omission");
            }
        }
    }

    if total_actions > limit {
        writeln!(
            output,
            "\n{} additional actions omitted (increase --limit to view)",
            total_actions - limit
        )
        .expect("write omission note");
    }

    output
}

fn render_dispatch_summary(
    output: &mut String,
    dispatch: &RemediationDispatch,
    now: u64,
) -> std::fmt::Result {
    write!(
        output,
        "    - {} target={} status={}",
        format_timestamp_with_relative(Some(dispatch.dispatched_at), now),
        dispatch.target,
        dispatch.status
    )?;
    if let Some(annotation) = dispatch.annotation.as_deref() {
        write!(output, " annotation={annotation}")?;
    }
    if let Some(ack) = dispatch.acknowledgement.as_ref() {
        let mut parts = Vec::new();
        if let Some(state) = ack.state.as_deref() {
            parts.push(state.to_string());
        }
        if let Some(flag) = ack.acknowledged {
            parts.push(format!("acknowledged={flag}"));
        }
        if let Some(flag) = ack.closed {
            parts.push(format!("closed={flag}"));
        }
        if let Some(notes) = ack.notes.as_deref() {
            if !notes.is_empty() {
                parts.push(format!("notes: {notes}"));
            }
        }
        if let Some(timestamp) = ack.timestamp {
            parts.push(format!(
                "timestamp={}",
                format_timestamp_with_relative(Some(timestamp), now)
            ));
        }
        if !parts.is_empty() {
            write!(output, " ack={}", parts.join(", "))?;
        }
    }
    output.push('\n');
    Ok(())
}

fn group_dispatches(
    dispatches: Vec<RemediationDispatch>,
) -> HashMap<DispatchKey, Vec<RemediationDispatch>> {
    let mut map: HashMap<DispatchKey, Vec<RemediationDispatch>> = HashMap::new();
    for dispatch in dispatches {
        map.entry(dispatch.key())
            .or_insert_with(Vec::new)
            .push(dispatch);
    }
    for entries in map.values_mut() {
        entries.sort_by(|a, b| b.dispatched_at.cmp(&a.dispatched_at));
    }
    map
}

fn parse_dispatch_ack(value: Option<&JsonValue>) -> AnyhowResult<Option<DispatchAck>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    if matches!(raw, JsonValue::Null) {
        return Ok(None);
    }
    let object = raw
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("dispatch acknowledgement must be an object"))?;
    Ok(Some(DispatchAck {
        state: object
            .get("state")
            .and_then(JsonValue::as_str)
            .map(ToOwned::to_owned),
        acknowledged: object.get("acknowledged").and_then(JsonValue::as_bool),
        closed: object.get("closed").and_then(JsonValue::as_bool),
        notes: object
            .get("notes")
            .and_then(JsonValue::as_str)
            .map(ToOwned::to_owned),
        timestamp: object.get("timestamp").and_then(JsonValue::as_u64),
    }))
}

fn parse_labels(value: Option<&JsonValue>) -> Vec<RemediationLabel> {
    let Some(array) = value.and_then(JsonValue::as_array) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|entry| {
            let map = entry.as_object()?;
            let key = map.get("key")?.as_str()?;
            let value = map.get("value")?.as_str()?;
            Some(RemediationLabel {
                key: key.to_string(),
                value: value.to_string(),
            })
        })
        .collect()
}

fn parse_string_array(value: Option<&JsonValue>) -> Vec<String> {
    let Some(array) = value.and_then(JsonValue::as_array) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn required_string(object: &json::Map, key: &str, context: &str) -> AnyhowResult<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{context} missing '{key}' field"))
}

fn required_u64(object: &json::Map, key: &str, context: &str) -> AnyhowResult<u64> {
    object
        .get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| anyhow::anyhow!("{context} missing '{key}' field"))
}

fn current_unix_timestamp() -> u64 {
    match UtcDateTime::now().unix_timestamp() {
        Ok(ts) if ts >= 0 => ts as u64,
        _ => 0,
    }
}

fn format_timestamp_with_relative(ts: Option<u64>, now: u64) -> String {
    match ts {
        Some(value) => {
            let iso = format_timestamp(value);
            if value == now {
                format!("{iso} (now)")
            } else if value < now {
                format!("{iso} ({} ago)", format_duration(now - value))
            } else {
                format!("{iso} (in {})", format_duration(value - now))
            }
        }
        None => "-".to_string(),
    }
}

fn format_timestamp(value: u64) -> String {
    match UtcDateTime::from_unix_timestamp(value as i64) {
        Ok(dt) => dt.format_iso8601().unwrap_or_else(|_| value.to_string()),
        Err(_) => value.to_string(),
    }
}

fn format_duration(seconds: u64) -> String {
    let mut remaining = seconds;
    let mut parts = Vec::new();
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
    if remaining > 0 || parts.is_empty() {
        parts.push(format!("{}s", remaining));
    }
    parts.join(" ")
}

struct RemediationAction {
    action: String,
    playbook: String,
    peer_id: String,
    metric: String,
    labels: Vec<RemediationLabel>,
    occurrences: u64,
    delta: f64,
    ratio: f64,
    timestamp: u64,
    acknowledged_at: Option<u64>,
    closed_out_at: Option<u64>,
    acknowledgement_notes: Option<String>,
    first_dispatch_at: Option<u64>,
    last_dispatch_at: Option<u64>,
    pending_since: Option<u64>,
    pending_escalated: bool,
    dispatch_attempts: u64,
    auto_retry_count: u64,
    last_auto_retry_at: Option<u64>,
    last_ack_state: Option<String>,
    last_ack_notes: Option<String>,
    follow_up_notes: Option<String>,
    response_sequence: Vec<String>,
    dashboard_panels: Vec<String>,
    runbook_path: Option<String>,
    dispatch_endpoint: Option<String>,
}

impl RemediationAction {
    fn dispatch_key(&self) -> DispatchKey {
        DispatchKey {
            timestamp: self.timestamp,
            action: self.action.clone(),
            playbook: self.playbook.clone(),
            peer_id: self.peer_id.clone(),
            metric: self.metric.clone(),
        }
    }
}

#[derive(Clone)]
struct RemediationDispatch {
    action_timestamp: u64,
    action_kind: String,
    playbook: String,
    peer_id: String,
    metric: String,
    target: String,
    status: String,
    dispatched_at: u64,
    acknowledgement: Option<DispatchAck>,
    annotation: Option<String>,
}

impl RemediationDispatch {
    fn key(&self) -> DispatchKey {
        DispatchKey {
            timestamp: self.action_timestamp,
            action: self.action_kind.clone(),
            playbook: self.playbook.clone(),
            peer_id: self.peer_id.clone(),
            metric: self.metric.clone(),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct DispatchKey {
    timestamp: u64,
    action: String,
    playbook: String,
    peer_id: String,
    metric: String,
}

#[derive(Clone)]
struct DispatchAck {
    state: Option<String>,
    acknowledged: Option<bool>,
    closed: Option<bool>,
    notes: Option<String>,
    timestamp: Option<u64>,
}

#[derive(Clone)]
struct RemediationLabel {
    key: String,
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_report_includes_retry_and_dispatch_history() {
        let action = RemediationAction {
            action: "escalate".to_string(),
            playbook: "governance-escalation".to_string(),
            peer_id: "bridge-node".to_string(),
            metric: "bridge_settlement_results_total".to_string(),
            labels: vec![RemediationLabel {
                key: "asset".to_string(),
                value: "eth".to_string(),
            }],
            occurrences: 4,
            delta: 120.0,
            ratio: 4.2,
            timestamp: 2_000,
            acknowledged_at: Some(2_040),
            closed_out_at: None,
            acknowledgement_notes: Some("pager".to_string()),
            first_dispatch_at: Some(2_010),
            last_dispatch_at: Some(2_020),
            pending_since: Some(2_010),
            pending_escalated: true,
            dispatch_attempts: 1,
            auto_retry_count: 0,
            last_auto_retry_at: None,
            last_ack_state: Some("pending".to_string()),
            last_ack_notes: Some("waiting".to_string()),
            follow_up_notes: Some("Automated escalation queued".to_string()),
            response_sequence: vec!["step one".to_string()],
            dashboard_panels: vec![
                "bridge_remediation_action_total (5m delta)".to_string(),
                "bridge_remediation_dispatch_total (5m delta)".to_string(),
                "bridge_remediation_ack_latency_seconds (p50/p95)".to_string(),
            ],
            runbook_path: Some("docs/runbook".to_string()),
            dispatch_endpoint: Some("/remediation/bridge/dispatches".to_string()),
        };
        let dispatch = RemediationDispatch {
            action_timestamp: 2_000,
            action_kind: "escalate".to_string(),
            playbook: "governance-escalation".to_string(),
            peer_id: "bridge-node".to_string(),
            metric: "bridge_settlement_results_total".to_string(),
            target: "http".to_string(),
            status: "success".to_string(),
            dispatched_at: 2_015,
            acknowledgement: Some(DispatchAck {
                state: Some("pending".to_string()),
                acknowledged: Some(false),
                closed: Some(false),
                notes: Some("waiting".to_string()),
                timestamp: Some(2_018),
            }),
            annotation: Some("annotation".to_string()),
        };
        let rendered = render_bridge_report(vec![action], vec![dispatch], 5, 5);
        assert!(rendered.contains("Action 1"));
        assert!(rendered.contains("Dispatch history"));
        assert!(rendered.contains("Follow-ups"));
        assert!(rendered.contains("acknowledged=false"));
        assert!(rendered.contains("closed=false"));
        assert!(rendered.contains("bridge_remediation_ack_latency_seconds"));
    }
}
