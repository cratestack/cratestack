use super::without_source_positions;
use crate::Schema;

fn schema_from_json(json: &str) -> Schema {
    serde_json::from_str(json).expect("fixture schema deserializes")
}

fn fixture() -> Schema {
    let span = |start, end, line| serde_json::json!({ "start": start, "end": end, "line": line });
    let mut value = serde_json::to_value(schema_from_json(&empty_schema_json())).unwrap();
    value["datasource"] = serde_json::json!({
        "docs": ["the database"],
        "name": "db",
        "name_span": span(11, 13, 2),
        "entries": [{ "key": "provider", "value": "\"postgresql\"" }],
        "span": span(1, 90, 2),
    });
    value["auth"] = serde_json::json!({
        "docs": [],
        "name": "Caller",
        "name_span": span(100, 106, 7),
        "fields": [],
        "span": span(95, 130, 7),
    });
    serde_json::from_value(value).expect("fixture schema")
}

fn empty_schema_json() -> String {
    serde_json::to_string(&Schema {
        datasource: None,
        auth: None,
        config_blocks: Vec::new(),
        mixins: Vec::new(),
        models: Vec::new(),
        types: Vec::new(),
        enums: Vec::new(),
        procedures: Vec::new(),
        views: Vec::new(),
        queries: Vec::new(),
        subscription_procedures: Vec::new(),
        topics: Vec::new(),
        transport: Default::default(),
        declared_extensions: Default::default(),
        mcp: None,
    })
    .unwrap()
}

#[test]
fn every_span_is_zeroed_and_nothing_else_changes() {
    let schema = fixture();
    let zeroed = without_source_positions(&schema);

    let datasource = zeroed.datasource.as_ref().unwrap();
    assert_eq!(datasource.name_span.start, 0);
    assert_eq!(datasource.name_span.end, 0);
    assert_eq!(datasource.name_span.line, 0);
    assert_eq!(datasource.span.line, 0);
    assert_eq!(datasource.docs, ["the database"]);
    assert_eq!(datasource.name, "db");
    assert_eq!(datasource.entries[0].value, "\"postgresql\"");
    assert_eq!(zeroed.auth.as_ref().unwrap().span.end, 0);
    assert_eq!(zeroed.auth.as_ref().unwrap().name, "Caller");
    // The input is untouched.
    assert_eq!(schema.datasource.as_ref().unwrap().name_span.start, 11);
}

#[test]
fn zeroing_is_idempotent_and_equates_schemas_that_differ_only_in_position() {
    let shifted = {
        let mut value = serde_json::to_value(fixture()).unwrap();
        value["datasource"]["span"]["start"] = serde_json::json!(5000);
        value["datasource"]["span"]["line"] = serde_json::json!(42);
        serde_json::from_value::<Schema>(value).unwrap()
    };
    assert_ne!(fixture(), shifted);
    assert_eq!(
        without_source_positions(&fixture()),
        without_source_positions(&shifted)
    );
    let once = without_source_positions(&fixture());
    assert_eq!(without_source_positions(&once), once);
}
