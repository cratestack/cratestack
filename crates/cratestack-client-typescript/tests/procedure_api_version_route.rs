//! `@api_version` regression guard: every generated TypeScript layout must
//! call a versioned procedure at the path the server mounts,
//! `/<version>/$procs/<name>` (`cratestack-macros`'s `route_attrs`), not
//! the unversioned `/$procs/<name>` the server never registers.
//!
//! Before the fix `procedure_views::build_procedure` hardcoded
//! `/$procs/<name>`, so a procedure declared `@api_version("v2")` 404'd
//! from every layout (fetch client, TanStack keys, SWR, RTK). The route is
//! now derived through `cratestack_core::procedure_route`, the same
//! function the server mounts with. The end-to-end proof against a real
//! generated server is `cratestack-api`'s `api_version_client_round_trip.rs`.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

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

fn layouts() -> Vec<(&'static str, TypeScriptGeneratorConfig)> {
    let base = TypeScriptGeneratorConfig::default;
    vec![
        ("default", base()),
        (
            "tanstack",
            TypeScriptGeneratorConfig {
                tanstack: true,
                ..base()
            },
        ),
        (
            "swr",
            TypeScriptGeneratorConfig {
                swr: true,
                ..base()
            },
        ),
        (
            "rtk",
            TypeScriptGeneratorConfig {
                rtk: true,
                ..base()
            },
        ),
    ]
}

#[test]
fn every_layout_calls_the_versioned_procedure_at_its_mounted_path() {
    let schema = cratestack_parser::parse_schema(SCHEMA).expect("fixture should parse");
    for (layout, config) in layouts() {
        let package = generate_package(&schema, &config)
            .unwrap_or_else(|error| panic!("{layout} layout should render: {error}"));
        let mut versioned_hits = 0;
        for file in &package.files {
            let contents = &file.contents;
            versioned_hits += contents.matches("\"/v2/$procs/ping\"").count();
            assert!(
                !contents.contains("\"/$procs/ping\""),
                "{layout}: {} calls the unversioned /$procs/ping, which the server \
                 never mounts:\n{contents}",
                file.file_name,
            );
            assert!(
                !contents.contains("/v2/$procs/plain"),
                "{layout}: {} versioned the unversioned `plain` procedure",
                file.file_name,
            );
        }
        assert!(
            versioned_hits > 0,
            "{layout}: no generated file calls \"/v2/$procs/ping\""
        );
        assert!(
            package
                .files
                .iter()
                .any(|file| file.contents.contains("\"/$procs/plain\"")),
            "{layout}: the unversioned control must stay at /$procs/plain"
        );
    }
}
