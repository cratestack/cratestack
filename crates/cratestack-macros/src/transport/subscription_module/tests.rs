use std::collections::BTreeSet;

use super::generate_subscription_module;

const FEED: &str = r#"
transport rpc

model Widget {
  id Int @id
  name String
  @@allow("read", auth() != null)
  @@emit(created, deleted)
  @@subscribe
}
"#;

const NO_FEED: &str = r#"
model Widget {
  id Int @id
}
"#;

fn generate(source: &str, webtransport: bool) -> Result<String, String> {
    let schema = cratestack_parser::parse_schema(source).expect("schema should parse");
    let model_names: BTreeSet<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
    let enum_names = BTreeSet::new();
    generate_subscription_module(&schema.models, &model_names, &enum_names, webtransport)
        .map(|tokens| tokens.to_string())
}

#[test]
fn a_feed_gets_a_dispatcher_arm_on_the_backplane() {
    let code = generate(FEED, false).unwrap();
    assert!(code.contains("impl :: cratestack :: SubscriptionDispatcher for SubscriptionDispatch"));
    assert!(code.contains("\"model.Widget.subscribe\" => open_widget"));
    // Events come off the backplane (every replica), not this process's bus.
    assert!(code.contains("model_event_channel (\"Widget\")"));
    assert!(code.contains("model_events :: < super :: models :: Widget >"));
    assert!(!code.contains("on_widget_created"));
    assert!(code.contains("fn with_backplane"));
    assert!(code.contains("missing_policy_columns"));
    assert!(!code.contains("fn webtransport"));
}

#[test]
fn extension_webtransport_adds_the_server_constructor() {
    let code = generate(FEED, true).unwrap();
    assert!(code.contains("fn webtransport"));
    assert!(code.contains("WebTransportServer :: bind"));
}

#[test]
fn no_feed_generates_nothing() {
    assert_eq!(generate(NO_FEED, false).unwrap(), "");
}

#[test]
fn extension_webtransport_with_nothing_to_serve_is_an_error() {
    let error = generate(NO_FEED, true).unwrap_err();
    assert!(error.contains("no subscription op"), "{error}");
}
