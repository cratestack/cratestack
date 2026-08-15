// Static (CI-safe, no external tooling) coverage for `--swr` (issue #304,
// made additive by #591): file-set shape, per-model content, the
// ownership rule's shared/inline split, the relation-cycle fixture, and
// the framework-free claim (by text — see `tests/swr_runtime.rs` for the
// actual-Node-execution proof, which is best-effort/skippable since no
// Rust CI job in this repo currently provisions Node).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

#[test]
fn default_layout_output_is_unaffected_by_swr_existing() {
    // Belt-and-suspenders alongside the untouched `tests/snapshot.rs`:
    // this crate's default pipeline (`generator.rs::generate_default_package`)
    // is a separate code path from `crate::swr::generate`, so adding
    // `--swr` cannot have changed default output. Spot-check a couple of
    // default-only files still exist and `swr`-only files do not leak in
    // when the flag is off.
    let package = generate_for("tiny_rest", false);
    assert!(file_named(&package, "src/models.ts").is_some());
    assert!(file_named(&package, "src/client.ts").is_some());
    // Issue #617: `src/react-query.ts` is gated behind `--tanstack` now,
    // off by default like `swr` here — `generate_for`'s `..Default::default()`
    // leaves it unset.
    assert!(file_named(&package, "src/react-query.ts").is_none());
    assert!(file_named(&package, "src/swr/models/shared.ts").is_none());
    assert!(file_named(&package, "src/swr/procedures.ts").is_none());
}

#[test]
fn swr_rest_file_set_is_additive_to_the_default_layout() {
    let package = generate_for("tiny_rest", true);
    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "README.md",
            "package.json",
            "src/client.ts",
            "src/index.ts",
            "src/models.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "src/swr/index.ts",
            "src/swr/models/shared.ts",
            "src/swr/models/widget.hooks.ts",
            "src/swr/models/widget.ts",
            "src/swr/procedures.hooks.ts",
            "src/swr/procedures.ts",
            "src/swr/queries.ts",
            "src/swr/runtime.ts",
            "src/swr/swr-keys.ts",
            "tsconfig.json",
        ],
        "--swr's REST file set changed unexpectedly (the default layout must \
         stay present alongside src/swr/**; src/react-query.ts is absent here \
         because --tanstack, issue #617, is off in this fixture)"
    );
}

#[test]
fn swr_rpc_file_set_is_additive_to_the_default_layout() {
    let package = generate_for("tiny_rpc", true);
    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "README.md",
            "package.json",
            "src/cbor-item.ts",
            "src/cbor-seq.ts",
            "src/client.ts",
            "src/index.ts",
            "src/links.ts",
            "src/models.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "src/stream-terminal.ts",
            "src/swr/cbor-item.ts",
            "src/swr/cbor-seq.ts",
            "src/swr/index.ts",
            "src/swr/links.ts",
            "src/swr/models/shared.ts",
            "src/swr/models/widget.hooks.ts",
            "src/swr/models/widget.ts",
            "src/swr/procedures.hooks.ts",
            "src/swr/procedures.ts",
            "src/swr/queries.ts",
            "src/swr/runtime.ts",
            "src/swr/stream-terminal.ts",
            "src/swr/swr-keys.ts",
            "tsconfig.json",
        ],
        "--swr's RPC file set changed unexpectedly (the default layout must \
         stay present alongside src/swr/**; src/react-query.ts is absent here \
         because --tanstack, issue #617, is off in this fixture)"
    );
}

/// Issue #333: the `swr` preset's per-model plain function, its sibling
/// hook, and `swrKeys` must all use the typed `CratestackRpcListQuery`
/// (not a bare `Record<string, unknown>`), and forward it through
/// `toRpcListInput()` exactly once — inside the plain function, never
/// re-serialized again in the hook that wraps it.
#[test]
fn swr_rpc_list_uses_typed_query_builder() {
    let package = generate_for("tiny_rpc", true);

    let widget = file(&package, "src/swr/models/widget.ts");
    assert!(
        widget.contains(
            "import { toRpcListInput, type CratestackRpcListQuery } from \"../queries.js\";"
        ),
        "src/swr/models/widget.ts does not import the typed list-query builder:\n{widget}"
    );
    assert!(
        widget.contains("export async function listWidgets(\n  runtime: CratestackRpcRuntime,\n  query: CratestackRpcListQuery = {},"),
        "listWidgets is not typed as CratestackRpcListQuery:\n{widget}"
    );
    assert!(
        widget.contains("toRpcListInput(query)"),
        "listWidgets does not forward its query through toRpcListInput:\n{widget}"
    );

    let hooks = file(&package, "src/swr/models/widget.hooks.ts");
    assert!(
        hooks.contains("import type { CratestackRpcListQuery } from \"../queries.js\";"),
        "src/swr/models/widget.hooks.ts does not import CratestackRpcListQuery:\n{hooks}"
    );
    assert!(
        hooks.contains("query: CratestackRpcListQuery = {}"),
        "useWidgets is not typed as CratestackRpcListQuery:\n{hooks}"
    );
    assert!(
        !hooks.contains("Record<string, unknown>"),
        "src/swr/models/widget.hooks.ts still references the untyped Record shape:\n{hooks}"
    );

    let keys = file(&package, "src/swr/swr-keys.ts");
    assert!(
        keys.contains("list: (query: CratestackRpcListQuery = {})"),
        "swrKeys.model.Widget.list is not typed as CratestackRpcListQuery:\n{keys}"
    );
}

#[test]
fn swr_per_model_file_has_types_and_plain_functions() {
    let package = generate_for("tiny_rest", true);
    let widget = file(&package, "src/swr/models/widget.ts");

    assert!(widget.contains("export interface Widget {"));
    assert!(widget.contains("export interface CreateWidgetInput {"));
    assert!(widget.contains("export interface UpdateWidgetInput {"));
    assert!(widget.contains("export async function listWidgets("));
    assert!(widget.contains("export async function getWidget("));
    assert!(widget.contains("export async function createWidget("));
    assert!(widget.contains("export async function updateWidget("));
    assert!(widget.contains("export async function deleteWidget("));
    assert!(widget.contains("runtime: CratestackRuntime"));
}

#[test]
fn swr_per_model_functions_are_framework_free() {
    // Static proof half of AC #9 (issue #305) — the plain-function files
    // from #304 (`src/swr/models/<model>.ts`, `src/swr/procedures.ts`,
    // and every other non-`.hooks.ts` file under `src/swr/`) still carry
    // zero React/`swr` reference, even though hooks now exist elsewhere
    // in the package — see
    // `src/swr/mod.rs`'s module doc for why hooks live in a *sibling*
    // `.hooks.ts` file instead of being appended here: an `import useSWR
    // from "swr"` in the same file `getWidget` lives in would be eagerly
    // resolved the moment that file loads, regardless of which export an
    // importer asked for, breaking the zero-`node_modules` runtime proof
    // below. The runtime half (actually calling a plain function against
    // a stub server with no `swr`/`react` installed at all) is
    // `tests/swr_runtime.rs`.
    let package = generate_for("tiny_rest", true);
    for file in package.files.iter().filter(|f| {
        f.file_name.starts_with("src/swr/")
            && f.file_name.ends_with(".ts")
            && !f.file_name.ends_with(".hooks.ts")
    }) {
        // Checks the actual `import ... from "swr"/"react"` statement, not
        // bare mentions of the word "swr"/"react" in prose — several of
        // these files' own header comments legitimately explain *why*
        // they stay free of such an import (see e.g. `src/swr-keys.ts`'s
        // header, or this file's own doc comment above), which would
        // otherwise false-positive a plain substring check.
        assert!(
            !file.contents.contains("from \"react\"") && !file.contents.contains("from 'react'"),
            "{} must not import react:\n{}",
            file.file_name,
            file.contents
        );
        assert!(
            !file.contents.contains("from \"swr\"")
                && !file.contents.contains("from 'swr'")
                && !file.contents.contains("from \"swr/mutation\"")
                && !file.contents.contains("use client"),
            "{} must not import swr or reference a client-component directive:\n{}",
            file.file_name,
            file.contents
        );
    }
    // The swr subtree's own root index never re-exports a `.hooks` module
    // either (same reasoning, one level up: `import { CratestackRuntime }
    // from "<pkg>/swr"` must not force `swr` to resolve).
    let index = file(&package, "src/swr/index.ts");
    assert!(
        !index
            .lines()
            .any(|line| line.starts_with("export") && line.contains(".hooks")),
        "src/swr/index.ts must not re-export any .hooks module:\n{index}"
    );
}

#[test]
fn swr_model_hooks_file_is_a_sibling_of_the_plain_function_file() {
    // Issue #305 AC #1/#2/#3: every model gets a `useSWR`/`useSWRMutation`
    // hook per operation, emitted into a per-model file (not a whole-
    // schema dump), as a thin wrapper over the plain function of the same
    // operation — never a reimplementation of the fetch itself.
    let package = generate_for("tiny_rest", true);
    let hooks = file(&package, "src/swr/models/widget.hooks.ts");

    assert!(hooks.contains("export function useWidgets("));
    assert!(hooks.contains("export function useWidget("));
    assert!(hooks.contains("export function useCreateWidget("));
    assert!(hooks.contains("export function useUpdateWidget("));
    assert!(hooks.contains("export function useDeleteWidget("));
    // Thin wrapper: hooks call the imported plain functions, they don't
    // reimplement `runtime.get`/`.post`/etc. themselves.
    assert!(hooks.contains("() => listWidgets(runtime, options)"));
    assert!(!hooks.contains("runtime.get<"));
    assert!(!hooks.contains("runtime.post<"));
}

#[test]
fn swr_procedures_hooks_file_covers_query_and_mutation_kinds() {
    let package = generate_for("tiny_rest", true);
    let hooks = file(&package, "src/swr/procedures.hooks.ts");
    // `tiny_rest.cstack`'s `echoName` procedure — see its own fixture
    // file for `kind`.
    assert!(
        hooks.contains("export function useEchoNameQuery(")
            || hooks.contains("export function useEchoNameMutation(")
    );
}

#[test]
fn paged_model_imports_page_in_every_file_that_uses_it() {
    // Regression test: a `@@paged` model's `list_return_type` is
    // `Page<{Model}>` (`crate::views::build_model_api`), and a
    // procedure can directly return `Page<T>` too — both are literal-
    // inlined into generated signatures rather than imported as a named
    // model type, so the ownership graph (`src/swr/ownership.rs`) never
    // sees either as a consumer edge. Before this fix, every file below
    // used `Page<Widget>` with no `import type { Page }` anywhere,
    // which fails `tsc --noEmit` with `TS2304: Cannot find name 'Page'`
    // (see `paged_model_output_type_checks` for the real-compiler
    // proof).
    for fixture in ["swr_paged_model", "swr_paged_model_rpc"] {
        let package = generate_for(fixture, true);

        // `Widget` also has filterable fields, so its own `<Model>Where`
        // pulls in the shared filter/sort primitives too — `Page` rides
        // along in that same merged `import type { ... } from "./shared.js"`
        // line rather than getting a standalone one.
        let model = file(&package, "src/swr/models/widget.ts");
        assert!(
            model.contains("Page") && model.contains("from \"./shared.js\";"),
            "{fixture}: src/swr/models/widget.ts should import Page from ./shared:\n{model}"
        );
        assert!(model.contains("Page<Widget>"));

        let model_hooks = file(&package, "src/swr/models/widget.hooks.ts");
        assert!(
            model_hooks.contains("Page") && model_hooks.contains("from \"./shared.js\";"),
            "{fixture}: src/swr/models/widget.hooks.ts should import Page from ./shared:\n{model_hooks}"
        );
        assert!(model_hooks.contains("Page<Widget>"));

        let procedures = file(&package, "src/swr/procedures.ts");
        assert!(
            procedures.contains("import type { Page } from \"./models/shared.js\";"),
            "{fixture}: src/swr/procedures.ts should import Page from ./models/shared:\n{procedures}"
        );
        assert!(procedures.contains("Page<Widget>"));

        let procedures_hooks = file(&package, "src/swr/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("import type { Page } from \"./models/shared.js\";"),
            "{fixture}: src/swr/procedures.hooks.ts should import Page from ./models/shared:\n{procedures_hooks}"
        );
        assert!(procedures_hooks.contains("Page<Widget>"));
    }
}

#[test]
fn page_input_procedure_argument_imports_page_input_in_every_file_that_uses_it() {
    // Same gap as `paged_model_imports_page_in_every_file_that_uses_it`,
    // for `PageInput` argument fields instead of a `Page<T>` return type:
    // literal-inlined by `ts_type`'s generic fallback, invisible to the
    // ownership graph, so `src/swr/context.rs` has to add the import by
    // hand.
    for fixture in ["swr_page_input_procedure", "swr_page_input_procedure_rpc"] {
        let package = generate_for(fixture, true);

        let procedures = file(&package, "src/swr/procedures.ts");
        assert!(
            procedures.contains("import type { PageInput } from \"./models/shared.js\";"),
            "{fixture}: src/swr/procedures.ts should import PageInput from ./models/shared:\n{procedures}"
        );
        assert!(procedures.contains("page: PageInput"));

        // `procedures.hooks.ts` is a thin wrapper around the plain
        // function's already-typed `ListFeedArgs` (which itself carries
        // `page: PageInput`, asserted above) rather than re-declaring the
        // `PageInput` field inline — see this file's own header doc.
        let procedures_hooks = file(&package, "src/swr/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("ListFeedArgs"),
            "{fixture}: src/swr/procedures.hooks.ts should reference ListFeedArgs:\n{procedures_hooks}"
        );
    }
}

#[test]
fn find_many_procedure_argument_imports_post_find_many_in_every_file_that_uses_it() {
    // Same gap again, for `FindMany<Model>` argument fields: `ts_type`
    // resolves `FindMany<Post>` to the *per-model* derived name
    // `PostFindMany` (defined in that model's own file, not shared —
    // see `find_many_views.rs`), a consumer edge the ownership graph
    // never sees since `PostFindMany` isn't a declared `type`/`enum` —
    // so `src/swr/context.rs` has to add the import by hand.
    for fixture in ["swr_find_many_procedure", "swr_find_many_procedure_rpc"] {
        let package = generate_for(fixture, true);

        let procedures = file(&package, "src/swr/procedures.ts");
        assert!(
            procedures.contains("import type { PostFindMany } from \"./models/post.js\";"),
            "{fixture}: src/swr/procedures.ts should import PostFindMany from ./models/post:\n{procedures}"
        );
        assert!(procedures.contains("query: PostFindMany"));

        let procedures_hooks = file(&package, "src/swr/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("SearchPostsArgs"),
            "{fixture}: src/swr/procedures.hooks.ts should reference SearchPostsArgs:\n{procedures_hooks}"
        );
    }
}

#[test]
fn swr_package_json_declares_swr_and_react_as_peer_dependencies() {
    // AC #8: `swr` (and the `react` it needs) are *peer* dependencies —
    // consumers who never import a `.hooks` module don't need them
    // installed at all.
    let package = generate_for("tiny_rest", true);
    let package_json = file(&package, "package.json");
    assert!(package_json.contains("\"peerDependencies\""));
    assert!(package_json.contains("\"swr\": \"^2.2.0\""));
    assert!(package_json.contains("\"react\":"));
}

#[test]
fn swr_package_json_dev_dependency_react_range_matches_the_peer_range() {
    // Real bug found while running the issue #306 example app for real in
    // a browser: the generated `devDependencies.react` used to be pinned
    // to exactly `^18.0.0` while `peerDependencies.react` allows `^18.0.0
    // || ^19.0.0`. In a pnpm workspace where the *consuming* app depends
    // on React 19 (as any new app reasonably would — this generated
    // package's own devDependency has nothing to do with what a real
    // consumer installs), pnpm cannot dedupe two non-overlapping ranges
    // and installs two separate React copies — one in the generated
    // package's own `node_modules` (18.x, to satisfy its devDependency),
    // one in the consumer's (19.x). Two React instances in one bundle is
    // the textbook "Invalid hook call" runtime crash, reproduced for real
    // (`useBoards`/`useCreateBoard` etc. threw immediately on mount) while
    // building `examples/react-vite-swr`. `devDependencies` must offer
    // the same range as `peerDependencies` so pnpm's resolver can settle
    // on one shared version whenever a consumer's own React version is
    // anywhere in that range.
    let package = generate_for("tiny_rest", true);
    let package_json = file(&package, "package.json");
    assert!(
        package_json.contains("\"react\": \"^18.0.0 || ^19.0.0\""),
        "devDependencies.react must allow the same range as peerDependencies \
         (^18.0.0 || ^19.0.0), not a single pinned major, or pnpm/npm can install \
         two incompatible React copies in a consumer's workspace:\n{package_json}"
    );
}

#[test]
fn swr_package_json_declares_subpath_exports_for_hooks_modules() {
    // Real bug found while building the end-to-end example app for issue
    // #306, still true after #591 nested the layout under `src/swr/`: the
    // package's own README tells consumers to import hooks via a subpath
    // — `<pkg>/swr/models/<model>.hooks` — because `.hooks` files are
    // deliberately not re-exported from the `src/swr/index.ts` barrel
    // (see `src/swr/mod.rs`'s module doc). A package.json with an
    // `exports` map blocks every subpath *not* listed in it (Node's
    // package-exports encapsulation, honored by bundlers and by
    // TypeScript's `Bundler`/`node16`/`nodenext` module resolution, all
    // three of which this generator's own `tsconfig.json.j2` and every JS
    // example in this repo use). Without this, `import { useWidget } from
    // "<pkg>/swr/models/widget.hooks"` fails to resolve for every real
    // consumer, even though the README told them to write exactly that.
    let package = generate_for("tiny_rest", true);
    let package_json = file(&package, "package.json");
    assert!(
        package_json.contains("\"./swr\""),
        "package.json must export the \"./swr\" subpath barrel"
    );
    assert!(
        package_json.contains("\"./swr/models/*\""),
        "package.json must export a subpath pattern covering src/swr/models/*.ts \
         and src/swr/models/*.hooks.ts, or subpath imports the README itself \
         recommends can't resolve"
    );
    assert!(package_json.contains("\"./swr/procedures\""));
    assert!(package_json.contains("\"./swr/procedures.hooks\""));
}

#[test]
fn swr_procedures_file_has_args_type_and_plain_function() {
    let package = generate_for("tiny_rest", true);
    let procedures = file(&package, "src/swr/procedures.ts");
    assert!(procedures.contains("export interface EchoNameArgs {"));
    assert!(procedures.contains("export async function echoName("));
    assert!(procedures.contains("runtime: CratestackRuntime"));
}

#[test]
fn swr_index_reexports_every_model_and_procedures() {
    let package = generate_for("tiny_rest", true);
    let index = file(&package, "src/swr/index.ts");
    assert!(index.contains("export * from \"./models/shared.js\";"));
    assert!(index.contains("export * from \"./models/widget.js\";"));
    assert!(index.contains("export * from \"./procedures.js\";"));
}

#[test]
fn swr_rejects_grpc_transport() {
    let schema =
        cratestack_parser::parse_schema_file("../../examples/grpc-widgets/schemas/widgets.cstack")
            .expect("grpc fixture should parse");
    let error = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect_err("--swr must reject transport grpc");
    assert!(matches!(
        error,
        cratestack_client_typescript::TypeScriptGeneratorError::SwrUnsupportedForGrpc
    ));
}

/// Acceptance test for the ownership rule (issue #304's self-review ask:
/// does this actually exercise the shared-vs-owned split, or could it
/// pass by accident on a trivial fixture?). `swr_shared_types.cstack`
/// has: `Status` (enum) used by two models, `Address` (a `type`, whose
/// only entry points are procedures — see the fixture's own header
/// comment and `src/swr/ownership.rs`'s module doc for why) used by two
/// procedures, and `Priority` (enum) used by exactly one model.
#[test]
fn cross_model_type_reuse_places_each_type_in_exactly_one_file() {
    let package = generate_for("swr_shared_types", true);
    let shared = file(&package, "src/swr/models/shared.ts");
    let project = file(&package, "src/swr/models/project.ts");
    let task = file(&package, "src/swr/models/task.ts");
    let procedures = file(&package, "src/swr/procedures.ts");

    // Status: shared, imported by both models, defined nowhere else.
    // Both models also pull in the shared filter/sort primitives for
    // their own `<Model>Where`, so `Status` rides along in the same
    // merged `import type { ... } from "./shared.js"` line rather than
    // getting a standalone one.
    assert!(shared.contains("export type Status ="));
    assert!(project.contains("Status") && project.contains("from \"./shared.js\";"));
    assert!(task.contains("Status") && task.contains("from \"./shared.js\";"));
    assert!(!project.contains("export type Status ="));
    assert!(!task.contains("export type Status ="));

    // Address: shared, imported by procedures.ts, defined nowhere else.
    assert!(shared.contains("export interface Address {"));
    assert!(procedures.contains("import type { Address } from \"./models/shared.js\";"));
    assert!(!procedures.contains("export interface Address {"));
    assert!(!project.contains("Address"));

    // Priority: owned solely by Task — inline there, absent from shared
    // and from Project.
    assert!(task.contains("export type Priority ="));
    assert!(!shared.contains("Priority"));
    assert!(!project.contains("Priority"));

    // No duplicate top-level type declarations anywhere within the swr
    // subtree. NOTE: this is scoped to `src/swr/**`, not the whole
    // package — the default layout's own `src/models.ts` legitimately
    // declares the very same types again (it does no shared/owned
    // partitioning at all; every type lands in that one file), and that
    // is correct, not a regression: `--swr` layers a second, independent
    // layout alongside the default one rather than deduplicating across
    // them, exactly like running the generator twice into two directories
    // used to.
    for name in ["Status", "Address", "Priority"] {
        let total_definitions: usize = package
            .files
            .iter()
            .filter(|f| f.file_name.starts_with("src/swr/"))
            .map(|f| {
                f.contents.matches(&format!("export type {name} =")).count()
                    + f.contents
                        .matches(&format!("export interface {name} {{"))
                        .count()
            })
            .sum();
        assert_eq!(
            total_definitions, 1,
            "{name} must be defined exactly once within the src/swr/ subtree"
        );
    }
}

/// Acceptance test for the relation-cycle fixture (`User` -> `Post[]` ->
/// `User`, AC #9): both model files typecheck-import each other, always
/// as `import type` — never a value import — so there is no runtime
/// import cycle, only a type-only one.
#[test]
fn relation_cycle_uses_type_only_cross_imports_with_no_value_level_cycle() {
    let package = generate_for("swr_relation_cycle", true);
    let user = file(&package, "src/swr/models/user.ts");
    let post = file(&package, "src/swr/models/post.ts");

    assert!(
        user.contains("import type { Post } from \"./post.js\";"),
        "user.ts must import Post as a type-only import:\n{user}"
    );
    assert!(
        post.contains("import type { User } from \"./user.js\";"),
        "post.ts must import User as a type-only import:\n{post}"
    );
    // Not a value import of the sibling model anywhere — grep for a
    // bare `import {` (no `type`) naming the other model's symbol.
    assert!(
        !user.contains("import { Post }") && !user.contains("import { Post,"),
        "user.ts must never value-import Post:\n{user}"
    );
    assert!(
        !post.contains("import { User }") && !post.contains("import { User,"),
        "post.ts must never value-import User:\n{post}"
    );
}

/// This test used to be the acceptance test for issue #305 AC #4 ("A
/// shared, exported key factory produces cache keys ... stable and
/// collision-free across models ... prove with a fixture designed to
/// collide"): it generated the `swr` preset for `swr_key_collision.cstack`
/// (`UserGroup`/`User_Group`) and asserted `src/swr-keys.ts` nested both
/// models under their own literal, parser-unique name rather than the
/// colliding `to_camel_case`-derived key. That assertion was true, but
/// blind to a second, worse collision the very same fixture triggers:
/// `crate::naming::split_words` is the shared tokenizer behind
/// `to_camel_case` *and* `to_kebab_case` (`to_pascal_case`/`to_snake_case`
/// too), so a pair that collapses to the same word sequence for one
/// collides for all of them. Pre-#344, that meant `swr` generation for
/// this exact fixture *looked* clean (this test passed) while silently
/// clobbering `src/models/user-group.ts`: both models' `SwrModelFileContext`
/// shared the file stem `user-group`, so the second model processed
/// overwrote the first's file on disk with no error — the old version of
/// this test never rendered or inspected `src/models/*.ts` at all, only
/// `src/swr-keys.ts`, so it could not have caught it.
///
/// `reject_model_file_name_collisions` (`src/swr/mod.rs`) now catches
/// this before any file is rendered and fails generation outright, naming
/// both colliding models — which means this exact fixture can no longer
/// reach `src/swr/swr-keys.ts` under `--swr` at all. This test now
/// proves that stronger, correct behavior instead. (The DEFAULT layout's
/// unrelated flat-object collision on the same fixture is unaffected —
/// see `default_layout_disambiguates_colliding_query_keys` below.)
#[test]
fn swr_rejects_colliding_model_file_names() {
    let fixture_path = "tests/fixtures/swr_key_collision.cstack";
    let schema = cratestack_parser::parse_schema_file(fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    let error = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect_err("--swr must reject two models whose kebab-case file names collide");

    match error {
        cratestack_client_typescript::TypeScriptGeneratorError::SwrModelFileNameCollision {
            first,
            second,
            file_stem,
        } => {
            assert_eq!(file_stem, "user-group");
            let names = [first.as_str(), second.as_str()];
            assert!(
                names.contains(&"UserGroup"),
                "error should name UserGroup: {names:?}"
            );
            assert!(
                names.contains(&"User_Group"),
                "error should name User_Group: {names:?}"
            );
        }
        other => panic!("expected SwrModelFileNameCollision, got {other:?}"),
    }
}

/// Found while implementing #305's `swr-keys.ts` (see this file's
/// `swr_key_factory_keeps_similarly_named_models_distinct`, whose docs
/// explain why `swr-keys.ts` nests under raw model names rather than
/// reusing `ModelApiView::list_query_key` &c.): the DEFAULT/react-query
/// preset's `views::build_model_api` derived those same fields purely
/// per-model, via `to_camel_case(&model.name)` — a lossy transform two
/// distinct, parser-guaranteed-unique model names (`UserGroup` and
/// `User_Group`) can collapse onto identically. `rest-react-query.ts.j2`/
/// `rpc-react-query.ts.j2` render `list_query_key`/`get_query_key` as
/// sibling property names in the same `cratestackQueryKeys` object
/// literal, so an undetected collision was a genuine TypeScript compile
/// error (`ts(1117)`), not just a cache-key overlap.
/// `views::disambiguate_model_api_keys` now runs once per schema, after
/// every model's `ModelApiView` is built, and suffixes any colliding key
/// with its own model's raw name (parser-unique, so the suffixed key is
/// guaranteed unique too).
#[test]
fn default_layout_disambiguates_colliding_query_keys() {
    // `src/react-query.ts` needs `--tanstack` (issue #617) now — this test
    // is about ITS content, not `--swr`'s, so `generate_for` (swr-only)
    // isn't the right helper here.
    let package = generate_for_tanstack("swr_key_collision");
    let react_query = file(&package, "src/react-query.ts");

    // No more literal duplicate property name.
    assert_eq!(
        react_query.matches("userGroupList:").count(),
        0,
        "the bare, undisambiguated `userGroupList:` property should no longer appear once \
         collisions are suffixed:\n{react_query}"
    );

    // Each model gets its own distinct, suffixed property for every one
    // of the five derived keys — proving the disambiguation covers
    // list/get (object-literal properties, the compile-error risk) and
    // create/update/delete (mutationKey array values, a lower-severity
    // but same-root-cause TanStack cache-key overlap).
    for suffix in ["List", "Detail", "Create", "Update", "Delete"] {
        let user_group_key = format!("userGroup{suffix}_UserGroup");
        let user_underscore_group_key = format!("userGroup{suffix}_User_Group");
        assert!(
            react_query.contains(&user_group_key),
            "missing disambiguated key {user_group_key:?}:\n{react_query}"
        );
        assert!(
            react_query.contains(&user_underscore_group_key),
            "missing disambiguated key {user_underscore_group_key:?}:\n{react_query}"
        );
    }
}

fn generate_for(fixture_stem: &str, swr: bool) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("generation should succeed for this fixture/flag combination")
}

/// Like `generate_for`, but turns on `--tanstack` (swr off) instead of
/// `--swr` — for the one test in this file that's actually about
/// `src/react-query.ts`'s own content (query-key disambiguation), not
/// about `--swr`'s file set.
fn generate_for_tanstack(fixture_stem: &str) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            tanstack: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("generation should succeed for this fixture/flag combination")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    file_named(package, file_name).unwrap_or_else(|| panic!("missing generated file {file_name}"))
}

fn file_named<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> Option<&'a str> {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .map(|file| file.contents.as_str())
}
