use contract_cli::tx::FeeLane;
use contract_cli::wallet::{
    preview_build_tx_report, BuildTxStatus, FeeFloorPreviewError, Language, Localizer,
    SignerMetadata, SignerSource,
};

#[test]
fn auto_bump_emits_warning_event() {
    let localizer = Localizer::new(Language::En);
    let (report, event) = preview_build_tx_report(
        FeeLane::Consumer,
        "alice",
        "bob",
        100,
        2,
        100,
        0,
        &[],
        true,
        false,
        false,
        &localizer,
        10,
        SignerSource::Local,
    )
    .expect("preview");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(report.auto_bumped);
    assert!(!report.forced);
    assert_eq!(report.effective_fee, 10);
    assert_eq!(report.fee_floor, 10);
    assert!(report.payload.is_some());
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(
        report.signer_metadata,
        Some(vec![SignerMetadata {
            signer: "alice".to_string(),
            source: SignerSource::Local,
        }])
    );
    let event = event.expect("telemetry event");
    assert_eq!(event.kind, "warning");
    assert_eq!(event.lane, FeeLane::Consumer);
    assert_eq!(event.fee, 10);
    assert_eq!(event.floor, 10);
}

#[test]
fn force_records_override_metric() {
    let localizer = Localizer::new(Language::En);
    let (report, event) = preview_build_tx_report(
        FeeLane::Consumer,
        "carol",
        "dave",
        200,
        5,
        100,
        1,
        &[],
        false,
        true,
        false,
        &localizer,
        50,
        SignerSource::Local,
    )
    .expect("preview");
    assert_eq!(report.status, BuildTxStatus::Ready);
    assert!(!report.auto_bumped);
    assert!(report.forced);
    assert_eq!(report.effective_fee, 5);
    assert_eq!(report.fee_floor, 50);
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(
        report.signer_metadata,
        Some(vec![SignerMetadata {
            signer: "carol".to_string(),
            source: SignerSource::Local,
        }])
    );
    let event = event.expect("telemetry event");
    assert_eq!(event.kind, "override");
    assert_eq!(event.lane, FeeLane::Consumer);
    assert_eq!(event.fee, 5);
    assert_eq!(event.floor, 50);
}

#[test]
fn preview_requires_prompt_when_no_flags() {
    let localizer = Localizer::new(Language::En);
    let result = preview_build_tx_report(
        FeeLane::Consumer,
        "erin",
        "frank",
        10,
        1,
        100,
        0,
        &[],
        false,
        false,
        false,
        &localizer,
        9,
        SignerSource::Local,
    );
    assert!(matches!(result, Err(FeeFloorPreviewError::PromptRequired)));
}
