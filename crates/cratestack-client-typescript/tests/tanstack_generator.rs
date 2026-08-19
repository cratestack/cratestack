//! `--tanstack` (issue #617): gates `src/react-query.ts` (TanStack Query
//! `useQuery`/`useMutation` hooks over the default layout's client class),
//! its `src/index.ts` re-export, and the `@tanstack/react-query` peer/dev
//! dependency behind an additive flag — mirroring `--swr` (#589) and
//! `--refine` (#571), which this issue's own intent section describes as
//! "finishing the convergence" those two already went through. Before this
//! flag existed, all three were emitted unconditionally for every schema
//! and every transport.
//!
//! Structural coverage only (source-level assertions, no `tsc`/`npm`) — see
//! `tests/swr_paged_model_tsc.rs`/`tests/node_dist_esm.rs` for this crate's
//! established "real compiler" pattern (best-effort, skips when
//! `node`/`npm`/`npx` aren't on `PATH`, since no Rust CI job here
//! provisions Node).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

const REST_FIXTURE: &str = "tiny_rest";
const RPC_FIXTURE: &str = "tiny_rpc";

#[test]
fn without_the_flag_react_query_is_absent_everywhere_it_used_to_appear() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(fixture, Flags::default());

        assert!(
            file_named(&package, "src/react-query.ts").is_none(),
            "{fixture}: src/react-query.ts must not be emitted without --tanstack"
        );
        let index = file(&package, "src/index.ts");
        assert!(
            !index.contains("react-query"),
            "{fixture}: src/index.ts must not mention react-query without --tanstack:\n{index}"
        );
        let package_json = file(&package, "package.json");
        assert!(
            !package_json.contains("@tanstack/react-query"),
            "{fixture}: package.json must not mention @tanstack/react-query without \
             --tanstack:\n{package_json}"
        );
        let readme = file(&package, "README.md");
        assert!(
            !readme.contains("TanStack"),
            "{fixture}: README.md must not document TanStack Query hooks that aren't \
             emitted without --tanstack:\n{readme}"
        );
    }
}

#[test]
fn the_flag_emits_the_hooks_file_re_export_and_dependency() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let package = generate(
            fixture,
            Flags {
                tanstack: true,
                ..Flags::default()
            },
        );

        let react_query = file(&package, "src/react-query.ts");
        assert!(react_query.contains("useQuery"), "{fixture}: {react_query}");
        assert!(
            react_query.contains("useMutation"),
            "{fixture}: {react_query}"
        );

        let index = file(&package, "src/index.ts");
        assert!(
            index.contains("export * from \"./react-query.js\";"),
            "{fixture}: src/index.ts should re-export react-query.ts under --tanstack:\n{index}"
        );

        let package_json = file(&package, "package.json");
        // Both halves matter, same reasoning as `--refine`'s equivalent
        // assertion: the peer declares the consumer's obligation, the dev
        // dep is what makes `npm install && tsc` work in the generated
        // package on its own.
        assert_eq!(
            package_json
                .matches("\"@tanstack/react-query\": \"^5.0.0\"")
                .count(),
            2,
            "{fixture}: expected @tanstack/react-query in both peerDependencies and \
             devDependencies:\n{package_json}"
        );
    }
}

#[test]
fn the_flag_is_additive_every_other_file_is_byte_identical() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        let plain = generate(fixture, Flags::default());
        let with_tanstack = generate(
            fixture,
            Flags {
                tanstack: true,
                ..Flags::default()
            },
        );

        assert!(
            !plain
                .files
                .iter()
                .any(|f| f.file_name == "src/react-query.ts"),
            "{fixture}: src/react-query.ts must not be emitted without the flag"
        );

        for file in &plain.files {
            let counterpart = with_tanstack
                .files
                .iter()
                .find(|candidate| candidate.file_name == file.file_name)
                .unwrap_or_else(|| panic!("{fixture}: --tanstack dropped {}", file.file_name));
            // `package.json` and `src/index.ts` legitimately differ (the
            // dependency and the re-export), same carve-out
            // `tests/refine_generator.rs` uses for `--refine`. `README.md`
            // additionally differs here — unlike `--refine`/`--swr`, which
            // added a wholly new section describing a wholly new file,
            // `--tanstack` gates an existing README section that predates
            // the flag (`templates/README.md.j2`'s intro line and its
            // "## TanStack Query" section), so leaving it unconditional
            // would ship a README documenting hooks that don't exist.
            if matches!(
                file.file_name.as_str(),
                "package.json" | "src/index.ts" | "README.md"
            ) {
                continue;
            }
            assert_eq!(
                file.contents, counterpart.contents,
                "{fixture}: --tanstack changed {} — it must only ADD a file",
                file.file_name
            );
        }
    }
}

/// The issue's own Test Plan: the flag matrix `{}`, `{--tanstack}`,
/// `{--swr}`, `{--tanstack --swr}`, `{--refine}`, `{--tanstack --refine}`,
/// across REST and RPC — proving `--tanstack` composes freely in both
/// directions rather than merely not crashing when combined.
#[test]
fn composes_with_swr_and_refine_in_every_combination() {
    for fixture in [REST_FIXTURE, RPC_FIXTURE] {
        for swr in [false, true] {
            for refine in [false, true] {
                for tanstack in [false, true] {
                    let package = generate(
                        fixture,
                        Flags {
                            swr,
                            refine,
                            tanstack,
                        },
                    );
                    let combo = format!("{fixture} swr={swr} refine={refine} tanstack={tanstack}");

                    assert_eq!(
                        file_named(&package, "src/react-query.ts").is_some(),
                        tanstack,
                        "{combo}: src/react-query.ts presence should track --tanstack exactly"
                    );
                    assert_eq!(
                        file_named(&package, "src/refine.ts").is_some(),
                        refine,
                        "{combo}: src/refine.ts presence should track --refine exactly"
                    );
                    assert_eq!(
                        file_named(&package, "src/swr/index.ts").is_some(),
                        swr,
                        "{combo}: src/swr/index.ts presence should track --swr exactly"
                    );

                    // The decisive structural proof that the join-with-
                    // separator rewrite (`crate::package_deps`) produces
                    // syntactically valid JSON in every corner of this
                    // matrix, not just the two `tests/snapshot.rs` pins —
                    // a real `serde_json::from_str` parse, not a substring
                    // check, so a stray/missing comma actually fails this.
                    let package_json = file(&package, "package.json");
                    serde_json::from_str::<serde_json::Value>(package_json).unwrap_or_else(
                        |error| {
                            panic!(
                                "{combo}: package.json is not valid JSON: {error}\n{package_json}"
                            )
                        },
                    );
                }
            }
        }
    }
}

#[derive(Default)]
struct Flags {
    swr: bool,
    refine: bool,
    tanstack: bool,
}

fn generate(fixture_stem: &str, flags: Flags) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "tanstack-fixture-client".to_owned(),
            swr: flags.swr,
            refine: flags.refine,
            tanstack: flags.tanstack,
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
