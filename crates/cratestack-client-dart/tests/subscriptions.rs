//! Slice S6: the generated Dart client's subscription surface. The running
//! proof is `fixtures/subscriptions_rpc_test.dart` under `just verify-dart`;
//! this pins what the generator emits, and that a schema without a
//! subscription op emits none of it.

use cratestack_client_dart::{DartGeneratorConfig, generate_package};

fn files(fixture: &str) -> (String, String) {
    let schema =
        cratestack_parser::parse_schema_file(fixture).expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "subscriptions_client".to_owned(),
            native_cbor: false,
            ..DartGeneratorConfig::default()
        },
    )
    .expect("default template should render");
    let file = |name: &str| {
        package
            .files
            .iter()
            .find(|file| file.file_name == name)
            .unwrap_or_else(|| panic!("{name} generated"))
            .contents
            .clone()
    };
    (file("lib/src/apis.dart"), file("lib/src/runtime.dart"))
}

#[test]
fn a_schema_with_subscriptions_gets_streams_over_sse() {
    let (apis, runtime) = files("tests/fixtures/subscriptions_rpc.cstack");
    assert!(
        apis.contains("Stream<CratestackModelEvent<Note>> subscribe({"),
        "{apis}"
    );
    assert!(apis.contains("opId: 'model.Note.subscribe',"), "{apis}");
    assert!(apis.contains("ssePath: '/rpc/subscribe/model.Note.subscribe',"));
    assert!(apis.contains("input: input.toWire(),"));
    assert!(apis.contains("socketPath: '/rpc/ws',"));
    assert!(apis.contains("Stream<Line> lines(LinesArgs args, {"));
    assert!(apis.contains("cratestackProcedureQuery(args.toWire())"));
    assert!(apis.contains("return cratestackSubscribe("));
    assert!(apis.contains("this.subscriptions = const CratestackSubscriptionOptions()"));
    assert!(runtime.contains("Stream<Object?> cratestackSubscribeSse("));
    assert!(runtime.contains("final class CratestackSubscriptionException"));
}

#[test]
fn a_rest_schema_subscribes_at_its_escaped_routes() {
    let (apis, _) = files("../cratestack-pg/tests/fixtures/subscribe_rest.cstack");
    assert!(
        apis.contains("ssePath: '/sub_notes/\\$subscribe',")
            && apis.contains("socketPath: '/\\$ws',"),
        "{apis}"
    );
    let (apis, _) = files("../cratestack-pg/tests/fixtures/subscription_procedures_rest.cstack");
    assert!(
        apis.contains("ssePath: '/\\$procs/roomMessages',"),
        "{apis}"
    );
}

#[test]
fn a_schema_without_subscriptions_emits_none_of_it() {
    let (apis, runtime) = files("tests/fixtures/ci_rpc.cstack");
    assert!(!apis.contains("subscribe"), "{apis}");
    assert!(!runtime.contains("cratestackSubscribeSse"));
}

#[test]
fn the_socket_modules_come_only_with_subscriptions() {
    let names = |fixture: &str| {
        let schema = cratestack_parser::parse_schema_file(fixture).expect("parses");
        generate_package(
            &schema,
            &DartGeneratorConfig {
                library_name: "subscriptions_client".to_owned(),
                native_cbor: false,
                ..DartGeneratorConfig::default()
            },
        )
        .expect("renders")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect::<std::collections::BTreeMap<_, _>>()
    };
    let with = names("tests/fixtures/subscriptions_rpc.cstack");
    let sockets = &with["lib/src/subscription_sockets.dart"];
    assert!(sockets.contains("if (dart.library.js_interop) 'subscription_webtransport_web.dart'"));
    assert!(with["lib/src/subscription_webtransport_web.dart"].contains("web.WebTransport("));
    assert!(with["lib/src/subscription_webtransport_stub.dart"].contains("=> false;"));
    assert!(with["lib/src/runtime.dart"].contains("export 'subscription_sockets.dart';"));
    assert!(with["pubspec.yaml"].contains("web_socket_channel:"));
    assert!(with["pubspec.yaml"].contains("\n  web: "));

    let without = names("tests/fixtures/ci_rpc.cstack");
    assert!(!without.keys().any(|name| name.contains("subscription_")));
    assert!(!without["pubspec.yaml"].contains("web_socket_channel"));
}
