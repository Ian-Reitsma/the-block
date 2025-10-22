use contract_cli::tx::FeeLane;
use contract_cli::wallet::{
    preview_build_tx_report, BuildTxStatus, Language, Localizer, SignerMetadata, SignerSource,
};
use foundation_serialization::json::{Map as JsonMap, Value as JsonValue};

fn metadata_entry(signer: &str, source: &str) -> JsonMap {
    let mut map = JsonMap::new();
    map.insert("signer".to_owned(), JsonValue::String(signer.to_owned()));
    map.insert("source".to_owned(), JsonValue::String(source.to_owned()));
    map
}

fn metadata_snapshot(report: &contract_cli::wallet::BuildTxReport) -> Vec<JsonMap> {
    report
        .signer_metadata
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let label = match entry.source {
                        SignerSource::Local => "local",
                        SignerSource::Ephemeral => "ephemeral",
                        SignerSource::Session => "session",
                    };
                    metadata_entry(&entry.signer, label)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn preview_ready_reports_local_signer_metadata() {
    let localizer = Localizer::new(Language::En);
    let (report, event) = preview_build_tx_report(
        FeeLane::Consumer,
        "alice",
        "bob",
        50,
        10,
        100,
        1,
        &[],
        false,
        false,
        true,
        &localizer,
        7,
        SignerSource::Local,
    )
    .expect("preview");

    assert_eq!(report.status, BuildTxStatus::Ready);
    assert_eq!(report.warnings.len(), 0);
    assert!(event.is_none());
    assert_eq!(
        report.signer_metadata,
        Some(vec![SignerMetadata {
            signer: "alice".to_owned(),
            source: SignerSource::Local,
        }])
    );
    assert_eq!(metadata_snapshot(&report), vec![metadata_entry("alice", "local")]);
}

#[test]
fn preview_auto_bump_preserves_ephemeral_metadata_and_event() {
    let localizer = Localizer::new(Language::En);
    let (report, event) = preview_build_tx_report(
        FeeLane::Consumer,
        "ephemeral",
        "recipient",
        100,
        5,
        80,
        2,
        b"memo",
        true,
        false,
        true,
        &localizer,
        25,
        SignerSource::Ephemeral,
    )
    .expect("preview");

    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.auto_bumped);
    assert_eq!(report.effective_fee, 25);
    assert_eq!(
        report.signer_metadata,
        Some(vec![SignerMetadata {
            signer: "ephemeral".to_owned(),
            source: SignerSource::Ephemeral,
        }])
    );
    assert_eq!(
        metadata_snapshot(&report),
        vec![metadata_entry("ephemeral", "ephemeral")]
    );

    let telemetry = event.expect("telemetry event");
    assert_eq!(telemetry.kind, "warning");
    assert_eq!(telemetry.lane, FeeLane::Consumer);
    assert_eq!(telemetry.fee, 25);
    assert_eq!(telemetry.floor, 25);
}

#[test]
fn preview_session_signer_metadata_snapshot() {
    let localizer = Localizer::new(Language::En);
    let (report, _) = preview_build_tx_report(
        FeeLane::Industrial,
        "session-signer",
        "beneficiary",
        64,
        12,
        50,
        9,
        b"session memo",
        false,
        false,
        true,
        &localizer,
        12,
        SignerSource::Session,
    )
    .expect("preview");

    assert_eq!(report.status, BuildTxStatus::Ready);
    assert_eq!(
        report.signer_metadata,
        Some(vec![SignerMetadata {
            signer: "session-signer".to_owned(),
            source: SignerSource::Session,
        }])
    );
    assert_eq!(
        metadata_snapshot(&report),
        vec![metadata_entry("session-signer", "session")]
    );
}
