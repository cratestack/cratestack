//! `--rtk` (issue #906): gates `src/rtk-api.ts` (a typed RTK Query
//! `createApi` endpoint set dispatching through `@cratestack/adapter-rtk`
//! on RPC / this package's own REST client on REST), its `src/index.ts`
//! re-export, and the `@reduxjs/toolkit`/`react-redux`/
//! `@cratestack/adapter-rtk` peer + dev dependencies behind an additive
//! flag — the same shape `--tanstack` (#617) established.
//!
//! Structural coverage only (source-level assertions, no `tsc`/`npm`) —
//! see `tests/rtk_typecheck.rs` for the real-compiler proof, and
//! `tests/rtk_tag_derivation.rs` for the schema-derived tag proof.

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";

#[test]
fn without_the_flag_rtk_api_is_absent_everywhere() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(fixture, Flags::default());

        assert!(
            file_named(&package, "src/rtk-api.ts").is_none(),
            "{fixture}: src/rtk-api.ts must not be emitted without --rtk"
        );
        let index = file(&package, "src/index.ts");
        assert!(
            !index.contains("rtk-api"),
            "{fixture}: src/index.ts must not mention rtk-api without --rtk:\n{index}"
        );
        let package_json = file(&package, "package.json");
        for name in ["@reduxjs/toolkit", "react-redux", "@cratestack/adapter-rtk"] {
            assert!(
                !package_json.contains(name),
                "{fixture}: package.json must not mention {name} without --rtk:\n{package_json}"
            );
        }
    }
}

#[test]
fn the_flag_emits_the_endpoint_file_re_export_and_dependencies() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(
            fixture,
            Flags {
                rtk: true,
                ..Flags::default()
            },
        );

        let rtk_api = file(&package, "src/rtk-api.ts");
        assert!(
            rtk_api.contains("createCratestackRtkApi"),
            "{fixture}: {rtk_api}"
        );
        assert!(
            rtk_api.contains("createApi"),
            "{fixture}: must import/call RTK Query's createApi:\n{rtk_api}"
        );
        assert!(
            rtk_api.contains("listWidget: builder.query"),
            "{fixture}: should declare a list endpoint for the fixture's Widget model:\n{rtk_api}"
        );

        let index = file(&package, "src/index.ts");
        assert!(
            index.contains("export * from \"./rtk-api.js\";"),
            "{fixture}: src/index.ts should re-export rtk-api under --rtk:\n{index}"
        );

        let package_json = file(&package, "package.json");
        for name in ["@reduxjs/toolkit", "react-redux"] {
            assert_eq!(
                package_json.matches(&format!("\"{name}\"")).count(),
                2,
                "{fixture}: expected {name} in both peerDependencies and \
                 devDependencies:\n{package_json}"
            );
        }
    }
}

/// RPC transport additionally depends on `@cratestack/adapter-rtk`; REST
/// never does — see `crate::rtk`'s module doc for why.
#[test]
fn only_rpc_transport_declares_the_adapter_rtk_dependency() {
    let rest_package = generate(
        REST_FIXTURE,
        Flags {
            rtk: true,
            ..Flags::default()
        },
    );
    let rest_package_json = file(&rest_package, "package.json");
    assert!(
        !rest_package_json.contains("@cratestack/adapter-rtk"),
        "REST transport must not depend on @cratestack/adapter-rtk — it has no base-query \
         seam to dispatch through:\n{rest_package_json}"
    );
    let rest_rtk_api = file(&rest_package, "src/rtk-api.ts");
    assert!(
        !rest_rtk_api.contains("from \"@cratestack/adapter-rtk\""),
        "REST src/rtk-api.ts must not import from @cratestack/adapter-rtk (a doc comment MAY \
         still mention the package name to explain why not):\n{rest_rtk_api}"
    );
    assert!(
        rest_rtk_api.contains("fakeBaseQuery"),
        "REST src/rtk-api.ts should use RTK Query's fakeBaseQuery placeholder, since every \
         endpoint dispatches through queryFn calling this package's own REST client:\n{rest_rtk_api}"
    );

    let rpc_package = generate(
        RPC_FIXTURE,
        Flags {
            rtk: true,
            ..Flags::default()
        },
    );
    let rpc_package_json = file(&rpc_package, "package.json");
    assert_eq!(
        rpc_package_json
            .matches("\"@cratestack/adapter-rtk\"")
            .count(),
        2,
        "RPC transport should depend on @cratestack/adapter-rtk in both peerDependencies and \
         devDependencies:\n{rpc_package_json}"
    );
    let rpc_rtk_api = file(&rpc_package, "src/rtk-api.ts");
    assert!(
        rpc_rtk_api.contains("createRpcBaseQuery"),
        "RPC src/rtk-api.ts must dispatch through @cratestack/adapter-rtk's \
         createRpcBaseQuery:\n{rpc_rtk_api}"
    );
}

#[test]
fn the_flag_is_additive_every_other_file_is_byte_identical() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let plain = generate(fixture, Flags::default());
        let with_rtk = generate(
            fixture,
            Flags {
                rtk: true,
                ..Flags::default()
            },
        );

        assert!(
            !plain.files.iter().any(|f| f.file_name == "src/rtk-api.ts"),
            "{fixture}: src/rtk-api.ts must not be emitted without the flag"
        );

        for file in &plain.files {
            let counterpart = with_rtk
                .files
                .iter()
                .find(|candidate| candidate.file_name == file.file_name)
                .unwrap_or_else(|| panic!("{fixture}: --rtk dropped {}", file.file_name));
            // `package.json`/`src/index.ts` legitimately differ (the
            // dependencies and the re-export), and `README.md` gains a new
            // "## RTK Query" section — same three-way carve-out
            // `tests/tanstack_generator.rs` uses for `--tanstack`.
            if matches!(
                file.file_name.as_str(),
                "package.json" | "src/index.ts" | "README.md"
            ) {
                continue;
            }
            assert_eq!(
                file.contents, counterpart.contents,
                "{fixture}: --rtk changed {} — it must only ADD a file",
                file.file_name
            );
        }
    }
}

/// The flag matrix, mirroring `tests/tanstack_generator.rs`'s
/// `composes_with_swr_and_refine_in_every_combination`: proves `--rtk`
/// composes freely with every other flag rather than merely not crashing
/// when combined, and that every combination still produces valid JSON.
#[test]
fn composes_with_every_other_flag_in_every_combination() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        for swr in [false, true] {
            for refine in [false, true] {
                for tanstack in [false, true] {
                    for rtk in [false, true] {
                        let package = generate(
                            fixture,
                            Flags {
                                swr,
                                refine,
                                tanstack,
                                rtk,
                            },
                        );
                        let combo = format!(
                            "{fixture} swr={swr} refine={refine} tanstack={tanstack} rtk={rtk}"
                        );

                        assert_eq!(
                            file_named(&package, "src/rtk-api.ts").is_some(),
                            rtk,
                            "{combo}: src/rtk-api.ts presence should track --rtk exactly"
                        );

                        let package_json = file(&package, "package.json");
                        serde_json::from_str::<serde_json::Value>(package_json)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "{combo}: package.json is not valid JSON: {error}\n{package_json}"
                                )
                            });
                    }
                }
            }
        }
    }
}

/// ADR 0019: RTK Query builds a cache key from a `bigint` argument with its
/// default `serializeQueryArgs`, which throws on one before `@reduxjs/toolkit`
/// 2.2.4 (reduxjs/redux-toolkit@ae838b4c, first in `v2.2.4`), and the generated
/// `rtk-api.ts` only emits declarations from 2.2.7 (TS2527 before it). The
/// runtime proof re-pins the toolkit to the floor declared here, builds the
/// package and runs it (`tests/bigint_query_keys.rs`).
#[test]
fn the_toolkit_floor_serializes_a_bigint_argument_and_builds_the_generated_api() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(
            fixture,
            Flags {
                rtk: true,
                ..Flags::default()
            },
        );
        let package_json = file(&package, "package.json");
        assert_eq!(
            package_json
                .matches("\"@reduxjs/toolkit\": \"^2.2.7\"")
                .count(),
            2,
            "{fixture}: expected @reduxjs/toolkit ^2.2.7 in both peerDependencies and \
             devDependencies:\n{package_json}"
        );
        assert!(
            !package_json.contains("\"@reduxjs/toolkit\": \"^2.0.0\""),
            "{fixture}: the pre-2.2.7 floor throws on a bigint argument or cannot build rtk-api.ts:\n{package_json}"
        );
    }
}

/// RTK tag ids are `string | number`, so a `bigint` key is tagged by its decimal
/// string, and the query that provides a tag and the mutation that invalidates
/// it must agree. `tsc` over the generated package (`tests/bigint_query_keys.rs`)
/// is what fails without this; the text pins it without Node.
#[test]
fn a_bigint_key_is_tagged_by_its_decimal_string_and_other_keys_are_untouched() {
    for rpc in [false, true] {
        let source = std::fs::read_to_string("tests/fixtures/bigint_query_keys.cstack")
            .expect("read the fixture");
        let source = if rpc {
            source.replacen("model Counter {", "transport rpc\n\nmodel Counter {", 1)
        } else {
            source
        };
        let schema = cratestack_parser::parse_schema(&source).expect("fixture should parse");
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: "bigint-rtk-fixture-client".to_owned(),
                rtk: true,
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .expect("--rtk should render");
        let rtk_api = file(&package, "src/rtk-api.ts");

        for needle in [
            "id: item.id?.toString() as string,",
            "[{ type: \"Counter\" as const, id: id.toString() }]",
            "{ type: \"Counter\" as const, id: arg.id.toString() },",
            // `Ledger` has a `Cuid` key: its tags are exactly what they were.
            "id: item.id as string,",
            "[{ type: \"Ledger\" as const, id: id }]",
            "{ type: \"Ledger\" as const, id: arg.id },",
        ] {
            assert!(
                rtk_api.contains(needle),
                "rpc={rpc}: expected `{needle}` in the generated rtk-api.ts:\n{rtk_api}"
            );
        }
        assert_eq!(
            rtk_api.matches("arg.id.toString()").count(),
            2,
            "rpc={rpc}: update and delete both invalidate the stringified tag:\n{rtk_api}"
        );
        assert!(
            rtk_api.contains("isPlain(value)") && rtk_api.contains("$bigint"),
            "rpc={rpc}: the serializability-check guidance for bigint arguments is documented:\n{rtk_api}"
        );
    }

    // An `Int` key is a `number` and keeps its raw tag id.
    let package = generate(
        REST_FIXTURE,
        Flags {
            rtk: true,
            ..Flags::default()
        },
    );
    let rtk_api = file(&package, "src/rtk-api.ts");
    assert!(
        !rtk_api.contains(".toString()"),
        "an Int key must not be stringified:\n{rtk_api}"
    );
}

#[derive(Default)]
struct Flags {
    swr: bool,
    refine: bool,
    tanstack: bool,
    rtk: bool,
}

fn generate(fixture_stem: &str, flags: Flags) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "rtk-fixture-client".to_owned(),
            swr: flags.swr,
            refine: flags.refine,
            tanstack: flags.tanstack,
            rtk: flags.rtk,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: generation should succeed: {error}"))
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    file_named(package, name).unwrap_or_else(|| panic!("generated package should contain {name}"))
}

fn file_named<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> Option<&'a str> {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
}
