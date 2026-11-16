//! The generated TypeScript client's half of a signed subscription open
//! (cratestack#1378), asserted on the generated text so it runs without
//! Node: a subscription procedure marked `@frames(unsigned)` carries
//! `frames: "unsigned"` in its target on both transports and an unmarked one
//! does not; the runtime knows the `seal` option and refuses locally
//! (`streams_unsupported`) what cannot be opened sealed; both readers take the
//! `fetch` to open with. The behaviour is run for real in
//! `sealed_subscriptions_runtime.rs`.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const BODY: &str = r#"
type Room {
  roomId String
}

type Line {
  body String
}

subscription procedure lines(args: Room): Line

subscription procedure pulse(args: Room): Line
  @frames(unsigned)
"#;

fn files(transport_rpc: bool) -> Vec<(String, String)> {
    let source = match transport_rpc {
        true => format!("transport rpc\n{BODY}"),
        false => BODY.to_owned(),
    };
    let schema = cratestack_parser::parse_schema(&source).expect("fixture should parse");
    generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("package should render")
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(file, _)| file == name)
        .map(|(_, contents)| contents.as_str())
        .unwrap_or_else(|| panic!("{name} was not generated"))
}

/// The `target` literal of the method named `method`.
fn target_of<'a>(client: &'a str, method: &str) -> &'a str {
    let from = client
        .find(&format!("  {method}(\n"))
        .unwrap_or_else(|| panic!("no {method} method"));
    let rest = &client[from..];
    let start = rest.find("const target").expect("a target");
    let end = rest[start..].find("};").expect("the target ends") + start;
    &rest[start..end]
}

#[test]
fn a_marked_procedure_carries_its_frames_mode_on_both_transports() {
    for rpc in [true, false] {
        let generated = files(rpc);
        let client = file(&generated, "src/client.ts");
        let pulse = target_of(client, "pulse");
        assert!(
            pulse.contains("frames: \"unsigned\","),
            "rpc={rpc}: {pulse}"
        );
        assert!(
            pulse.starts_with("const target: subscriptions.SubscriptionTarget = {"),
            "rpc={rpc}: the literal must keep its `\"unsigned\"` type: {pulse}"
        );
        let lines = target_of(client, "lines");
        assert!(!lines.contains("frames"), "rpc={rpc}: {lines}");
    }
}

#[test]
fn the_runtime_takes_a_sealer_and_refuses_what_it_cannot_seal() {
    for rpc in [true, false] {
        let generated = files(rpc);
        let subscriptions = file(&generated, "src/subscriptions.ts");
        assert!(
            subscriptions.contains("seal?: SubscriptionSeal;"),
            "rpc={rpc}"
        );
        assert!(
            subscriptions.contains("frames?: \"unsigned\";"),
            "rpc={rpc}"
        );
        let sealed = file(&generated, "src/subscription-sealed.ts");
        assert!(sealed.contains("\"streams_unsupported\""), "rpc={rpc}");
        assert!(sealed.contains("seal.fetchFor"), "rpc={rpc}");
        // A sealed client never opens a plain subscription: the sealed
        // branch comes before any binding is tried.
        let branch = subscriptions
            .find("openSealed(host, target, headers, options.signal, seal)")
            .expect("a sealed branch");
        let first_plain = subscriptions.find("openCborSeq(host, target, headers, options.signal)");
        assert!(first_plain.is_some_and(|plain| branch < plain), "rpc={rpc}");
        for reader in ["src/subscription-sse.ts", "src/subscription-cbor-seq.ts"] {
            let text = file(&generated, reader);
            assert!(text.contains("via: OpenVia"), "rpc={rpc} {reader}");
        }
        let open = file(&generated, "src/subscription-open.ts");
        assert!(open.contains("init.method = \"POST\";"), "rpc={rpc}");
        assert!(open.contains("encode(target.input)"), "rpc={rpc}");
        // An unsigned refusal's body is never read for a code.
        assert!(open.contains("\"unsigned_refusal\""), "rpc={rpc}");
        assert!(
            open.contains("Symbol.for(\"cratestack.unsignedRefusal\")"),
            "rpc={rpc}"
        );
    }
}
