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
            native_cbor: false,
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

const RPC: &str = "tests/fixtures/subscriptions_rpc.cstack";

#[test]
fn the_rpc_client_and_model_subscribe() {
    let client = file(RPC, "lib/src/client.dart");
    assert!(client.contains("Stream<Object?> subscribe("), "{client}");
    assert!(client.contains("'/rpc/subscribe/$opId'"));

    let model = file(RPC, "lib/src/models/note.dart");
    assert!(
        model.contains("Stream<CratestackModelEvent<Note>> subscribe({"),
        "{model}"
    );
    assert!(model.contains(".subscribe('model.Note.subscribe', input.toQuery()"));
    assert!(model.contains("@riverpod\nStream<CratestackModelEvent<Note>> noteEvents("));
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
    assert!(procedures.contains("@riverpod\nStream<Line> lines(Ref ref, LinesArgs args) {"));
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
        model.contains(".subscribe('/sub_notes/\\$subscribe', input.toQuery()"),
        "{model}"
    );
    let client = file(
        "../cratestack-pg/tests/fixtures/subscribe_rest.cstack",
        "lib/src/client.dart",
    );
    assert!(client.contains("'$basePath$path'"), "{client}");
}

#[test]
fn a_schema_without_subscriptions_emits_none_of_it() {
    for name in ["lib/src/client.dart", "lib/src/procedures.dart"] {
        let contents = file("tests/fixtures/ci_rpc.cstack", name);
        assert!(!contents.contains("subscribe"), "{name}: {contents}");
        assert!(!contents.contains("Stream<"), "{name}: {contents}");
    }
}
