//! cratestack#1378: the generated Dart client seals a subscription open only
//! when the app opts in, `CratestackSubscriptionOptions.sealsSubscriptions`
//! (default `false`). This pins what the generator emits for both modes; the
//! running proof is `fixtures/subscriptions_rpc_frames_test.dart` under
//! `just verify-dart`.

use cratestack_client_dart::{DartGeneratorConfig, generate_package};

fn file(name: &str) -> String {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/subscriptions_rpc.cstack")
        .expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "subscriptions_client".to_owned(),
            ..DartGeneratorConfig::default()
        },
    )
    .expect("default template should render");
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("{name} generated"))
        .contents
        .clone()
}

#[test]
fn the_opt_in_is_an_option_that_defaults_to_off() {
    let sockets = file("lib/src/subscription_sockets.dart");
    assert!(
        sockets.contains("    this.sealsSubscriptions = false,\n  });"),
        "{sockets}"
    );
    assert!(sockets.contains("final bool sealsSubscriptions;"));
}

#[test]
fn without_the_opt_in_the_readers_are_handed_no_frames_and_check_no_marker() {
    let sockets = file("lib/src/subscription_sockets.dart");
    // Off: the readers get `null`, so no `extra` and no marker check.
    assert!(
        sockets.contains(
            "frames: seals ? target.frames : null,\n        sealRoute: seals ? target.sealRoute : null);"
        ),
        "{sockets}"
    );
    let runtime = file("lib/src/runtime.dart");
    let check = &runtime[runtime
        .find("Future<void> cratestackCheckFrames(")
        .expect("the check")..];
    let first = check
        .find("if (frames == null) {\n    return;\n  }")
        .expect("a gate");
    let marker = check.find("cratestack-frames").expect("the marker read");
    assert!(
        first < marker,
        "an unsigned open must return before the marker is read"
    );
}

#[test]
fn with_the_opt_in_a_socket_pin_or_an_unmarked_target_is_refused_before_connecting() {
    let sockets = file("lib/src/subscription_sockets.dart");
    let refusal = sockets
        .find("code: 'streams_unsupported',")
        .expect("streams_unsupported");
    let http = sockets
        .find("Stream<Object?> http(bool cborSeq) =>")
        .expect("the HTTP binding");
    assert!(refusal < http, "the refusal must come before any binding");
    let gate = &sockets[sockets.find("if (seals &&").expect("a gate")..refusal];
    for needle in [
        "target.frames == null",
        "options.transport == CratestackSubscriptionTransport.webSocket",
        "options.transport == CratestackSubscriptionTransport.webTransport",
    ] {
        assert!(gate.contains(needle), "{needle}: {gate}");
    }
    // `auto` reads cbor-seq, and never a socket, once sealing.
    assert!(sockets.contains("CratestackSubscriptionTransport.auto when seals => http(true),"));
    let auto_seals = sockets.find("auto when seals").expect("auto when seals");
    let auto_plain = sockets
        .find("auto when options.credentials == null")
        .expect("auto without credentials");
    assert!(auto_seals < auto_plain);
}

#[test]
fn an_unsigned_refusal_is_never_read_for_a_code_and_an_opened_one_is() {
    let runtime = file("lib/src/runtime.dart");
    assert!(runtime.contains("final bool opened;"));
    assert!(runtime.contains("this.opened = false,"));
    let refusal = &runtime[runtime
        .find("Future<CratestackSubscriptionException> _cratestackSubscriptionRefusal(")
        .expect("the refusal reader")..];
    let unsigned = refusal
        .find("if (sealed && !response.opened) {")
        .expect("the unsigned branch");
    let read = refusal.find("response.bytes()").expect("the body read");
    assert!(
        unsigned < read,
        "the body is read only after the unsigned branch"
    );
    assert!(refusal.contains("code: 'unsigned_refusal',"));
    // A body whose `code` is not a string is not a `TypeError`.
    assert!(refusal.contains("code: code is String ? code : 'unknown',"));
    let end = refusal
        .find("/// A terminal `err` frame")
        .expect("the frame error");
    assert!(!refusal[..end].contains("as String?"), "{refusal}");
    let frame_error = &refusal[end..];
    assert!(frame_error.contains("code: code is String ? code : 'unknown',"));
}
