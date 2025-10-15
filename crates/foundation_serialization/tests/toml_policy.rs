#[test]
fn parse_table_exposes_sections() {
    let input = r#"
[settings]
max_depth = 2

[tiers]
strategic = ["app"]
replaceable = ["dep_a", "deep_dep"]
forbidden = ["dep_b"]

[licenses]
forbidden = ["AGPL-3.0"]
"#;

    let table = foundation_serialization::toml::parse_table(input).expect("parse policy table");

    let settings = table
        .get("settings")
        .and_then(|value| match value {
            foundation_serialization::toml::Value::Object(map) => Some(map),
            _ => None,
        })
        .expect("settings table present");

    assert_eq!(
        settings.get("max_depth").and_then(|value| match value {
            foundation_serialization::toml::Value::Number(number) => number.as_u64(),
            _ => None,
        }),
        Some(2)
    );

    let tiers = table
        .get("tiers")
        .and_then(|value| match value {
            foundation_serialization::toml::Value::Object(map) => Some(map),
            _ => None,
        })
        .expect("tiers table present");

    let forbidden = tiers
        .get("forbidden")
        .and_then(|value| match value {
            foundation_serialization::toml::Value::Array(values) => Some(values),
            _ => None,
        })
        .expect("forbidden array present");

    assert_eq!(forbidden.len(), 1);
    assert_eq!(
        forbidden[0],
        foundation_serialization::toml::Value::String("dep_b".to_string())
    );
}
