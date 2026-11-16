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
    // cratestack#1379: the SSE reader is bounded; the behaviour itself is
    // proven by `fixtures/subscriptions_rpc_test.dart` under `just verify-dart`.
    assert!(runtime.contains("final class CratestackEventTooLargeException"));
    assert!(runtime.contains("int maxEventBytes = cratestackDefaultMaxEventBytes,"));
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

/// cratestack#1001: `application/cbor-seq` is a pinned binding on every
/// schema with a subscription, REST and RPC alike; the behaviour is proven by
/// `fixtures/subscriptions_rpc_cbor_seq*_test.dart` under `just verify-dart`; the
/// `ready` gate's place in the reader is asserted here, since CI is the only
/// place Dart runs.
#[test]
fn the_cbor_seq_binding_is_pinned_not_automatic() {
    for fixture in [
        "tests/fixtures/subscriptions_rpc.cstack",
        "../cratestack-pg/tests/fixtures/subscribe_rest.cstack",
    ] {
        let (_, runtime) = files(fixture);
        assert!(
            runtime.contains("Stream<Object?> cratestackSubscribeCborSeq("),
            "{fixture}"
        );
        assert!(runtime.contains("'Accept': 'application/cbor-seq',"));
        assert!(runtime.contains("_cratestackSkipCborItem(buffer, offset, 0)"));
        assert!(runtime.contains("CratestackEventTooLargeException(maxEventBytes)"));
        assert!(runtime.contains("0xf6"), "the keep-alive null is skipped");
        assert!(runtime.contains("the subscription body ended in the middle of a frame"));
        // A stream must open with `ready`, and the media type is checked.
        assert!(runtime.contains("a subscription must start with a `ready` frame"));
        // The gate: its refusal comes after the `err` check (an `err` first is
        // the server's refusal) and before the frame is acted on, so a `next`
        // or `end` first is never yielded or taken as the end.
        let reader = &runtime[runtime
            .find("Stream<Object?> cratestackSubscribeCborSeq(")
            .expect("the reader")..];
        let gate = "if (!ready && frame['ready'] == null) {";
        let (gate_at, err_at, end_at, next_at) = (
            reader.find(gate).expect("the ready gate"),
            reader
                .find("final err = frame['err'];")
                .expect("the err check"),
            reader
                .find("if (frame['end'] != null) {")
                .expect("the end check"),
            reader.find("yield frame['next'];").expect("the next yield"),
        );
        assert!(
            err_at < gate_at && gate_at < end_at && end_at < next_at,
            "{fixture}"
        );
        assert!(runtime.contains("if (!ready) {\n          ready = true;"));
        assert!(runtime.contains("expected an application/cbor-seq subscription body"));
        assert!(runtime.contains("a subscription frame must be a CBOR map"));
        // A model event's id is a 16-byte string over cbor-seq.
        assert!(runtime.contains("cratestackDecodeUuid(map['event_id']"));
    }
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/subscriptions_rpc.cstack")
        .expect("parses");
    let package = generate_package(&schema, &DartGeneratorConfig::default()).expect("renders");
    let sockets = &package
        .files
        .iter()
        .find(|file| file.file_name == "lib/src/subscription_sockets.dart")
        .expect("sockets module")
        .contents;
    assert!(
        sockets.contains("webSocket, sse, cborSeq }"),
        "the enum gains a value"
    );
    assert!(sockets.contains("CratestackSubscriptionTransport.cborSeq => http(true),"));
    // `auto` is unchanged: it never lands on `cborSeq`.
    let auto = sockets
        .split("CratestackSubscriptionTransport.auto when")
        .nth(1)
        .expect("auto arm");
    assert!(!auto.contains("http(true)"), "auto must not pick cbor-seq");
}

#[test]
fn a_schema_without_subscriptions_has_no_cbor_seq_reader() {
    let (_, runtime) = files("tests/fixtures/ci_rpc.cstack");
    assert!(!runtime.contains("cratestackSubscribeCborSeq"));
    assert!(!runtime.contains("_cratestackSubscriptionRefusal"));
}

/// cratestack#1378: a subscription marked `@frames(unsigned)` carries its mode
/// and the route a signing transport binds; the readers hand the transport
/// those and the contract in `extra`, and refuse an answer that is not the
/// unsigned stream asked for. The running proof is
/// `fixtures/subscriptions_rpc_frames_test.dart` under `just verify-dart`.
#[test]
fn a_marked_subscription_carries_its_frames_mode_and_seal_route() {
    let (apis, runtime) = files("tests/fixtures/subscriptions_rpc.cstack");
    assert!(
        apis.contains(
            "socketPath: '/rpc/ws',\n            frames: 'unsigned',\n            sealRoute: 'subscribe/procedure.pulse',\n          ),"
        ),
        "{apis}"
    );
    // The unmarked procedure and the model feed carry neither.
    assert_eq!(apis.matches("frames: 'unsigned',").count(), 1, "{apis}");
    assert_eq!(apis.matches("sealRoute:").count(), 1, "{apis}");
    for needle in [
        "'cratestack.frames': frames,",
        "'cratestack.route': route,",
        "'cratestack.contract': contract,",
        "code: 'unsigned_frames_refused',",
        "Future<void> cratestackCheckFrames(",
    ] {
        assert!(runtime.contains(needle), "{needle}");
    }
    for (reader, stream) in [
        ("_cratestackSseItems(", "'text/event-stream'"),
        ("_cratestackCborSeqItems(", "'application/cbor-seq'"),
    ] {
        let body = &runtime[runtime.rfind(reader).expect("the reader")..];
        let refusal = body.find("_cratestackSubscriptionRefusal(response)");
        let check = body.find(&format!(
            "cratestackCheckFrames(response, frames, {stream})"
        ));
        assert!(
            body.contains("extra: cratestackFramesExtra(frames, sealRoute),")
                && refusal
                    .zip(check)
                    .is_some_and(|(refusal, check)| refusal < check),
            "{reader}: the marker is checked after a refusal and before a frame"
        );
    }
}

#[test]
fn a_marked_rest_subscription_binds_its_route_template() {
    let schema = cratestack_parser::parse_schema(
        "type Room {\n  roomId String\n}\n\ntype Line {\n  body String\n}\n\n\
         subscription procedure pulse(args: Room): Line\n  @frames(unsigned)\n",
    )
    .expect("parses");
    let package = generate_package(&schema, &DartGeneratorConfig::default()).expect("renders");
    let apis = &package
        .files
        .iter()
        .find(|file| file.file_name == "lib/src/apis.dart")
        .expect("apis")
        .contents;
    assert!(
        apis.contains("frames: 'unsigned',\n            sealRoute: '/\\$procs/pulse',"),
        "{apis}"
    );
}

#[test]
fn a_schema_without_a_marked_subscription_has_no_frames_in_its_targets() {
    let (apis, _) = files("../cratestack-pg/tests/fixtures/subscription_procedures_rest.cstack");
    assert!(!apis.contains("sealRoute"), "{apis}");
    let (_, runtime) = files("tests/fixtures/ci_rpc.cstack");
    assert!(!runtime.contains("cratestackCheckFrames"));
}
