//! `@api_version` regression guard: every generated Dart layout must call a
//! versioned procedure at the path the server mounts,
//! `/<version>/$procs/<name>` (`cratestack-macros`'s `route_attrs`), not the
//! unversioned `/$procs/<name>` the server never registers.
//!
//! Before the fix `builders_model::build_procedure` hardcoded
//! `/\$procs/<name>`, so a procedure declared `@api_version("v2")` 404'd
//! from both the default and the riverpod preset. The route is now derived
//! through `cratestack_core::procedure_route`, the same function the server
//! mounts with, and only then escaped for a Dart string literal (`$` is
//! Dart's interpolation sigil). The end-to-end proof against a real
//! generated server is `cratestack-api`'s `api_version_client_round_trip.rs`.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const SCHEMA: &str = r#"
type PingArgs {
  message String
}

type PingReply {
  echo String
}

procedure ping(args: PingArgs): PingReply
  @api_version("v2")

procedure plain(args: PingArgs): PingReply
"#;

#[test]
fn every_preset_calls_the_versioned_procedure_at_its_mounted_path() {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture should parse");
    for preset in [DartPreset::Default, DartPreset::Riverpod] {
        let config = DartGeneratorConfig {
            preset,
            ..DartGeneratorConfig::default()
        };
        let package = generate_package(&schema, &config)
            .unwrap_or_else(|error| panic!("{preset:?} preset should render: {error}"));
        let mut versioned_hits = 0;
        for file in &package.files {
            let contents = &file.contents;
            versioned_hits += contents.matches("'/v2/\\$procs/ping'").count();
            assert!(
                !contents.contains("'/\\$procs/ping'"),
                "{preset:?}: {} calls the unversioned /$procs/ping, which the server \
                 never mounts:\n{contents}",
                file.file_name,
            );
            assert!(
                !contents.contains("/v2/\\$procs/plain"),
                "{preset:?}: {} versioned the unversioned `plain` procedure",
                file.file_name,
            );
        }
        assert!(
            versioned_hits > 0,
            "{preset:?}: no generated file calls '/v2/\\$procs/ping'"
        );
        assert!(
            package
                .files
                .iter()
                .any(|file| file.contents.contains("'/\\$procs/plain'")),
            "{preset:?}: the unversioned control must stay at /$procs/plain"
        );
    }
}
