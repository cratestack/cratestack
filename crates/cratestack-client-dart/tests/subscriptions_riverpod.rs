//! Slice S6 in `--preset riverpod`: the client's `subscribe`, the model
//! API's and `ProceduresApi`'s `Stream` methods, and a `@riverpod` stream
//! provider for each, on both transports; none of it without a subscription
//! op. `flutter-riverpod`'s committed example pins the no-subscription case
//! byte for byte (`just regen-examples --check`).

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

fn file(fixture: &str, name: &str) -> String {
    let schema =
        cratestack_parser::parse_schema_file(fixture).expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "subscriptions_client".to_owned(),
            preset: DartPreset::Riverpod,
            ..DartGeneratorConfig::default()
        },
    )
    .expect("riverpod preset should render");
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("{name} generated"))
        .contents
        .clone()
}

/// `ProviderException` is exported only by `flutter_riverpod/misc.dart`, so
/// the function must defer to the public `ProviderContainer.defaultRetry`.
fn assert_retry_function(client: &str) {
    assert!(
        client.contains("Duration? cratestackSubscriptionRetry(int retryCount, Object error) {\n  if (error is CratestackEventTooLargeException) return null;\n  if (error is CratestackSubscriptionException &&\n      const <String>{'unsigned_frames_refused', 'streams_unsupported', 'unsigned_refusal'}\n          .contains(error.code)) {\n    return null;\n  }\n  final reason = error is CratestackSubscriptionException ? error.endReason : null;\n  if (reason != null && reason.isTerminal) return null;\n  final delay = ProviderContainer.defaultRetry(retryCount, error);"),
        "{client}"
    );
    assert!(!client.contains("ProviderException"), "{client}");
}

const RPC: &str = "tests/fixtures/subscriptions_rpc.cstack";

#[test]
fn the_rpc_client_and_model_subscribe() {
    let client = file(RPC, "lib/src/client.dart");
    assert!(client.contains("Stream<Object?> subscribe("), "{client}");
    assert!(client.contains("return cratestackSubscribe("));
    assert!(
        client.contains("SubscriptionOptionsProvider = Provider<CratestackSubscriptionOptions>")
    );
    assert!(client.contains("subscriptions: ref.watch("));
    // Riverpod 3 retries a failed provider unless told otherwise; an
    // oversized event is not transient, so it must not be retried.
    assert_retry_function(&client);

    let model = file(RPC, "lib/src/models/note.dart");
    assert!(
        model.contains("Stream<CratestackModelEvent<Note>> subscribe({"),
        "{model}"
    );
    assert!(model.contains("opId: 'model.Note.subscribe',"), "{model}");
    assert!(model.contains("socketPath: '/rpc/ws',"));
    assert!(model.contains("@Riverpod(retry: cratestackSubscriptionRetry)\nStream<CratestackModelEvent<Note>> noteEvents("));
    assert!(model.contains("NoteApiProvider).subscribe(input: input);"));
}

#[test]
fn a_subscription_procedure_gets_a_method_and_a_stream_provider() {
    let procedures = file(RPC, "lib/src/procedures.dart");
    assert!(
        procedures.contains("part 'procedures.g.dart';"),
        "{procedures}"
    );
    assert!(procedures.contains("const ProceduresApi(this._client);"));
    assert!(procedures.contains("Stream<Line> lines(LinesArgs args, {"));
    assert!(procedures.contains("cratestackProcedureQuery(args.toWire())"));
    assert!(procedures.contains("@Riverpod(retry: cratestackSubscriptionRetry)\nStream<Line> lines(Ref ref, LinesArgs args) {"));
    assert!(procedures.contains("class LinesArgs"));
    assert!(procedures.contains("class Room"));

    let client = file(RPC, "lib/src/client.dart");
    assert!(client.contains("ProceduresApi get procedures => ProceduresApi(this);"));
}

#[test]
fn a_rest_schema_subscribes_at_its_routes() {
    let model = file(
        "../cratestack-pg/tests/fixtures/subscribe_rest.cstack",
        "lib/src/models/sub_note.dart",
    );
    assert!(
        model.contains("ssePath: '/sub_notes/\\$subscribe',"),
        "{model}"
    );
    let client = file(
        "../cratestack-pg/tests/fixtures/subscribe_rest.cstack",
        "lib/src/client.dart",
    );
    assert!(client.contains("basePath: basePath,"), "{client}");
    assert_retry_function(&client);
    let model = file(
        "../cratestack-pg/tests/fixtures/subscribe_rest.cstack",
        "lib/src/models/sub_note.dart",
    );
    assert!(
        model.contains("@Riverpod(retry: cratestackSubscriptionRetry)\nStream<"),
        "{model}"
    );
}

#[test]
fn a_schema_without_subscriptions_emits_none_of_it() {
    for name in ["lib/src/client.dart", "lib/src/procedures.dart"] {
        let contents = file("tests/fixtures/ci_rpc.cstack", name);
        assert!(!contents.contains("subscribe"), "{name}: {contents}");
        assert!(!contents.contains("Stream<"), "{name}: {contents}");
    }
}

/// cratestack#1378: the riverpod `ProceduresApi` builds the same marked target
/// the plain client does.
#[test]
fn a_marked_subscription_procedure_carries_its_seal_route() {
    let procedures = file(RPC, "lib/src/procedures.dart");
    assert!(
        procedures
            .contains("frames: 'unsigned',\n            sealRoute: 'subscribe/procedure.pulse',"),
        "{procedures}"
    );
    assert_eq!(procedures.matches("sealRoute:").count(), 1, "{procedures}");
}
