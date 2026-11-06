use std::collections::BTreeSet;

use super::{SubscriptionSchema, generate_subscription_module};

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
    generate_subscription_module(&SubscriptionSchema {
        models: &schema.models,
        model_names: &model_names,
        enum_names: &enum_names,
        procedures: &schema.subscription_procedures,
        webtransport,
        websocket: false,
        rpc: true,
        postgres: true,
    })
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

const PROCEDURE: &str = r#"
type Room {
  roomId String
}

type Message {
  body String
}

subscription procedure roomMessages(args: Room): Message
  @allow(auth() != null)
"#;

#[test]
fn a_subscription_procedure_gets_an_arm_over_the_registry() {
    let code = generate(PROCEDURE, false).unwrap();
    assert!(
        code.contains("SubscriptionDispatcher for SubscriptionDispatch < R >"),
        "{code}"
    );
    assert!(code.contains("fn subscriptions < R : super :: procedures :: ProcedureRegistry >"));
    assert!(code.contains("\"procedure.roomMessages\" => open_procedure_room_messages (& self . db , & self . registry"));
    assert!(code.contains("room_messages :: authorize_with_db"));
    assert!(code.contains("procedure_input_query"));
    // No model feed, so no feed query parser.
    assert!(!code.contains("fn feed_query"));
}

#[test]
fn extension_websocket_adds_the_route_at_the_transports_path() {
    let schema = cratestack_parser::parse_schema(PROCEDURE).expect("schema should parse");
    let names = BTreeSet::new();
    let generate = |rpc: bool| {
        generate_subscription_module(&SubscriptionSchema {
            models: &schema.models,
            model_names: &names,
            enum_names: &names,
            procedures: &schema.subscription_procedures,
            webtransport: false,
            websocket: true,
            rpc,
            postgres: true,
        })
        .unwrap()
        .to_string()
    };
    assert!(generate(true).contains("websocket :: router (config . default_path (\"/rpc/ws\")"));
    assert!(generate(false).contains("default_path (\"/$ws\")"));
}
