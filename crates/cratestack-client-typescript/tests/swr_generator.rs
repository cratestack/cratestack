// Static (CI-safe, no external tooling) coverage for the `swr` preset
// (issue #304): file-set shape, per-model content, the ownership rule's
// shared/inline split, the relation-cycle fixture, and the
// framework-free claim (by text — see `tests/swr_runtime.rs` for the
// actual-Node-execution proof, which is best-effort/skippable since no
// Rust CI job in this repo currently provisions Node).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, TypeScriptPreset, generate_package,
};

#[test]
fn default_preset_output_is_unaffected_by_the_swr_preset_existing() {
    // Belt-and-suspenders alongside the untouched `tests/snapshot.rs`:
    // this crate's default pipeline (`generator.rs::generate_default_package`)
    // is a separate code path from `crate::swr::generate`, so adding the
    // `swr` preset cannot have changed default output. Spot-check a
    // couple of default-only files still exist and `swr`-only files do
    // not leak in.
    let package = generate_for("tiny_rest", TypeScriptPreset::Default);
    assert!(file_named(&package, "src/models.ts").is_some());
    assert!(file_named(&package, "src/client.ts").is_some());
    assert!(file_named(&package, "src/react-query.ts").is_some());
    assert!(file_named(&package, "src/models/shared.ts").is_none());
    assert!(file_named(&package, "src/procedures.ts").is_none());
}

#[test]
fn swr_rest_file_set_matches_the_expected_layout() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
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
            "src/index.ts",
            "src/models/shared.ts",
            "src/models/widget.hooks.ts",
            "src/models/widget.ts",
            "src/procedures.hooks.ts",
            "src/procedures.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "src/swr-keys.ts",
            "tsconfig.json",
        ],
        "swr preset's REST file set changed unexpectedly"
    );
}

#[test]
fn swr_rpc_file_set_matches_the_expected_layout() {
    let package = generate_for("tiny_rpc", TypeScriptPreset::Swr);
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
            "src/index.ts",
            "src/links.ts",
            "src/models/shared.ts",
            "src/models/widget.hooks.ts",
            "src/models/widget.ts",
            "src/procedures.hooks.ts",
            "src/procedures.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "src/stream-terminal.ts",
            "src/swr-keys.ts",
            "tsconfig.json",
        ],
        "swr preset's RPC file set changed unexpectedly"
    );
}

/// Issue #333: the `swr` preset's per-model plain function, its sibling
/// hook, and `swrKeys` must all use the typed `CratestackRpcListQuery`
/// (not a bare `Record<string, unknown>`), and forward it through
/// `toRpcListInput()` exactly once — inside the plain function, never
/// re-serialized again in the hook that wraps it.
#[test]
fn swr_rpc_list_uses_typed_query_builder() {
    let package = generate_for("tiny_rpc", TypeScriptPreset::Swr);

    let widget = file(&package, "src/models/widget.ts");
    assert!(
        widget.contains(
            "import { toRpcListInput, type CratestackRpcListQuery } from \"../queries.js\";"
        ),
        "src/models/widget.ts does not import the typed list-query builder:\n{widget}"
    );
    assert!(
        widget.contains("export async function listWidgets(\n  runtime: CratestackRpcRuntime,\n  query: CratestackRpcListQuery = {},"),
        "listWidgets is not typed as CratestackRpcListQuery:\n{widget}"
    );
    assert!(
        widget.contains("toRpcListInput(query)"),
        "listWidgets does not forward its query through toRpcListInput:\n{widget}"
    );

    let hooks = file(&package, "src/models/widget.hooks.ts");
    assert!(
        hooks.contains("import type { CratestackRpcListQuery } from \"../queries.js\";"),
        "src/models/widget.hooks.ts does not import CratestackRpcListQuery:\n{hooks}"
    );
    assert!(
        hooks.contains("query: CratestackRpcListQuery = {}"),
        "useWidgets is not typed as CratestackRpcListQuery:\n{hooks}"
    );
    assert!(
        !hooks.contains("Record<string, unknown>"),
        "src/models/widget.hooks.ts still references the untyped Record shape:\n{hooks}"
    );

    let keys = file(&package, "src/swr-keys.ts");
    assert!(
        keys.contains("list: (query: CratestackRpcListQuery = {})"),
        "swrKeys.model.Widget.list is not typed as CratestackRpcListQuery:\n{keys}"
    );
}

#[test]
fn swr_per_model_file_has_types_and_plain_functions() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let widget = file(&package, "src/models/widget.ts");

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
    // from #304 (`src/models/<model>.ts`, `src/procedures.ts`, and every
    // other non-`.hooks.ts` file) still carry zero React/`swr` reference,
    // even though hooks now exist elsewhere in the package — see
    // `src/swr/mod.rs`'s module doc for why hooks live in a *sibling*
    // `.hooks.ts` file instead of being appended here: an `import useSWR
    // from "swr"` in the same file `getWidget` lives in would be eagerly
    // resolved the moment that file loads, regardless of which export an
    // importer asked for, breaking the zero-`node_modules` runtime proof
    // below. The runtime half (actually calling a plain function against
    // a stub server with no `swr`/`react` installed at all) is
    // `tests/swr_runtime.rs`.
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    for file in package
        .files
        .iter()
        .filter(|f| f.file_name.ends_with(".ts") && !f.file_name.ends_with(".hooks.ts"))
    {
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
    // The root index never re-exports a `.hooks` module either (same
    // reasoning, one level up: `import { CratestackRuntime } from
    // "<pkg>"` must not force `swr` to resolve).
    let index = file(&package, "src/index.ts");
    assert!(
        !index
            .lines()
            .any(|line| line.starts_with("export") && line.contains(".hooks")),
        "src/index.ts must not re-export any .hooks module:\n{index}"
    );
}

#[test]
fn swr_model_hooks_file_is_a_sibling_of_the_plain_function_file() {
    // Issue #305 AC #1/#2/#3: every model gets a `useSWR`/`useSWRMutation`
    // hook per operation, emitted into a per-model file (not a whole-
    // schema dump), as a thin wrapper over the plain function of the same
    // operation — never a reimplementation of the fetch itself.
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let hooks = file(&package, "src/models/widget.hooks.ts");

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
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let hooks = file(&package, "src/procedures.hooks.ts");
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
        let package = generate_for(fixture, TypeScriptPreset::Swr);

        let model = file(&package, "src/models/widget.ts");
        assert!(
            model.contains("import type { Page } from \"./shared.js\";"),
            "{fixture}: src/models/widget.ts should import Page from ./shared:\n{model}"
        );
        assert!(model.contains("Page<Widget>"));

        let model_hooks = file(&package, "src/models/widget.hooks.ts");
        assert!(
            model_hooks.contains("import type { Page } from \"./shared.js\";"),
            "{fixture}: src/models/widget.hooks.ts should import Page from ./shared:\n{model_hooks}"
        );
        assert!(model_hooks.contains("Page<Widget>"));

        let procedures = file(&package, "src/procedures.ts");
        assert!(
            procedures.contains("import type { Page } from \"./models/shared.js\";"),
            "{fixture}: src/procedures.ts should import Page from ./models/shared:\n{procedures}"
        );
        assert!(procedures.contains("Page<Widget>"));

        let procedures_hooks = file(&package, "src/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("import type { Page } from \"./models/shared.js\";"),
            "{fixture}: src/procedures.hooks.ts should import Page from ./models/shared:\n{procedures_hooks}"
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
        let package = generate_for(fixture, TypeScriptPreset::Swr);

        let procedures = file(&package, "src/procedures.ts");
        assert!(
            procedures.contains("import type { PageInput } from \"./models/shared.js\";"),
            "{fixture}: src/procedures.ts should import PageInput from ./models/shared:\n{procedures}"
        );
        assert!(procedures.contains("page: PageInput"));

        // `procedures.hooks.ts` is a thin wrapper around the plain
        // function's already-typed `ListFeedArgs` (which itself carries
        // `page: PageInput`, asserted above) rather than re-declaring the
        // `PageInput` field inline — see this file's own header doc.
        let procedures_hooks = file(&package, "src/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("ListFeedArgs"),
            "{fixture}: src/procedures.hooks.ts should reference ListFeedArgs:\n{procedures_hooks}"
        );
    }
}

#[test]
fn find_many_procedure_argument_imports_find_many_in_every_file_that_uses_it() {
    // Same gap again, for `FindMany<Model>` argument fields: the client-
    // side `FindMany` type is deliberately non-generic (the wire shape
    // never depends on the model), literal-inlined by `ts_type`'s generic
    // fallback with no consumer edge to the model it wraps either — so
    // `src/swr/context.rs` has to add the import by hand.
    for fixture in ["swr_find_many_procedure", "swr_find_many_procedure_rpc"] {
        let package = generate_for(fixture, TypeScriptPreset::Swr);

        let procedures = file(&package, "src/procedures.ts");
        assert!(
            procedures.contains("import type { FindMany } from \"./models/shared.js\";"),
            "{fixture}: src/procedures.ts should import FindMany from ./models/shared:\n{procedures}"
        );
        assert!(procedures.contains("query: FindMany"));

        let procedures_hooks = file(&package, "src/procedures.hooks.ts");
        assert!(
            procedures_hooks.contains("SearchPostsArgs"),
            "{fixture}: src/procedures.hooks.ts should reference SearchPostsArgs:\n{procedures_hooks}"
        );
    }
}

#[test]
fn swr_package_json_declares_swr_and_react_as_peer_dependencies() {
    // AC #8: `swr` (and the `react` it needs) are *peer* dependencies —
    // consumers who never import a `.hooks` module don't need them
    // installed at all.
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
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
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
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
    // Real bug found while building the end-to-end example app for
    // issue #306: the package's own README (`swr-README.md.j2`) tells
    // consumers to import hooks via a subpath — `<pkg>/models/<model>.hooks`
    // — because `.hooks` files are deliberately not re-exported from the
    // root barrel (see `src/swr/mod.rs`'s module doc). But `package.json`
    // only declared `exports["."]`, and a package.json with an `exports`
    // map blocks every subpath *not* listed in it (Node's package-exports
    // encapsulation, honored by bundlers and by TypeScript's `Bundler`/
    // `node16`/`nodenext` module resolution, all three of which this
    // generator's own `tsconfig.json.j2` and every JS example in this
    // repo use). Without this, `import { useWidget } from
    // "<pkg>/models/widget.hooks"` fails to resolve for every real
    // consumer, even though the README told them to write exactly that.
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let package_json = file(&package, "package.json");
    assert!(
        package_json.contains("\"./models/*\""),
        "package.json must export a subpath pattern covering src/models/*.ts \
         and src/models/*.hooks.ts, or subpath imports the README itself \
         recommends can't resolve"
    );
    assert!(package_json.contains("\"./procedures\""));
    assert!(package_json.contains("\"./procedures.hooks\""));
}

#[test]
fn swr_procedures_file_has_args_type_and_plain_function() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let procedures = file(&package, "src/procedures.ts");
    assert!(procedures.contains("export interface EchoNameArgs {"));
    assert!(procedures.contains("export async function echoName("));
    assert!(procedures.contains("runtime: CratestackRuntime"));
}

#[test]
fn swr_index_reexports_every_model_and_procedures() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let index = file(&package, "src/index.ts");
    assert!(index.contains("export * from \"./models/shared.js\";"));
    assert!(index.contains("export * from \"./models/widget.js\";"));
    assert!(index.contains("export * from \"./procedures.js\";"));
}

#[test]
fn swr_preset_rejects_grpc_transport() {
    let schema =
        cratestack_parser::parse_schema_file("../../examples/grpc-widgets/schemas/widgets.cstack")
            .expect("grpc fixture should parse");
    let error = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            preset: TypeScriptPreset::Swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect_err("swr preset must reject transport grpc");
    assert!(matches!(
        error,
        cratestack_client_typescript::TypeScriptGeneratorError::SwrPresetUnsupportedForGrpc
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
    let package = generate_for("swr_shared_types", TypeScriptPreset::Swr);
    let shared = file(&package, "src/models/shared.ts");
    let project = file(&package, "src/models/project.ts");
    let task = file(&package, "src/models/task.ts");
    let procedures = file(&package, "src/procedures.ts");

    // Status: shared, imported by both models, defined nowhere else.
    assert!(shared.contains("export type Status ="));
    assert!(project.contains("import type { Status } from \"./shared.js\";"));
    assert!(task.contains("import type { Status } from \"./shared.js\";"));
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

    // No duplicate top-level type declarations anywhere in the package.
    for name in ["Status", "Address", "Priority"] {
        let total_definitions: usize = package
            .files
            .iter()
            .map(|f| {
                f.contents.matches(&format!("export type {name} =")).count()
                    + f.contents
                        .matches(&format!("export interface {name} {{"))
                        .count()
            })
            .sum();
        assert_eq!(
            total_definitions, 1,
            "{name} must be defined exactly once across the whole package"
        );
    }
}

/// Acceptance test for the relation-cycle fixture (`User` -> `Post[]` ->
/// `User`, AC #9): both model files typecheck-import each other, always
/// as `import type` — never a value import — so there is no runtime
/// import cycle, only a type-only one.
#[test]
fn relation_cycle_uses_type_only_cross_imports_with_no_value_level_cycle() {
    let package = generate_for("swr_relation_cycle", TypeScriptPreset::Swr);
    let user = file(&package, "src/models/user.ts");
    let post = file(&package, "src/models/post.ts");

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

/// Acceptance test for issue #305 AC #4 ("A shared, exported key factory
/// produces cache keys ... stable and collision-free across models ...
/// prove with a fixture designed to collide"). `swr_key_collision.cstack`
/// has two models (`UserGroup`, `User_Group`) whose react-query-oriented
/// `ModelApiView::list_query_key` field — `to_camel_case(name) + "List"`
/// — collapses to the identical string `"userGroupList"` for both (see
/// the fixture's own header comment for the exact mechanism), which
/// would silently overwrite one property in that preset's flat
/// `cratestackQueryKeys` object. `src/swr-keys.ts` nests keys under each
/// model's own literal, parser-unique schema name instead, so it must
/// keep both models as distinct entries with distinct emitted key
/// strings — this test proves that, not just that both models render at
/// all.
#[test]
fn swr_key_factory_keeps_similarly_named_models_distinct() {
    let package = generate_for("swr_key_collision", TypeScriptPreset::Swr);
    let keys = file(&package, "src/swr-keys.ts");

    // Both models get their own nested entry, keyed by their literal
    // schema name — not the (colliding) camelCase-derived name.
    assert!(
        keys.contains("UserGroup: {"),
        "missing UserGroup entry:\n{keys}"
    );
    assert!(
        keys.contains("User_Group: {"),
        "missing User_Group entry:\n{keys}"
    );

    // The actual emitted key strings stay distinct (dispatch-unique op
    // ids), proving there is no overwrite: each model's dotted op id
    // appears exactly twice — once in its own `list` key builder, once
    // in its own `listMatches` filter — and, critically, `UserGroup`'s
    // two occurrences are never counted against `User_Group`'s (a
    // substring match would be a false pass here, since "UserGroup" is
    // not a substring of "User_Group" or vice versa).
    assert_eq!(
        keys.matches("\"model.UserGroup.list\"").count(),
        2,
        "model.UserGroup.list key string should appear exactly twice (list + listMatches):\n{keys}"
    );
    assert_eq!(
        keys.matches("\"model.User_Group.list\"").count(),
        2,
        "model.User_Group.list key string should appear exactly twice (list + listMatches):\n{keys}"
    );

    // The DEFAULT preset's `react-query.ts` used to collide on this exact
    // fixture (`to_camel_case`-derived keys collapsed UserGroup/User_Group
    // to the same `userGroupList` property, a literal duplicate
    // object-key and TypeScript compile error) — `swr-keys.ts` sidesteps
    // that by nesting under each model's raw, parser-unique name instead
    // of reusing the lossy transform. See
    // `default_preset_disambiguates_colliding_query_keys` below for the
    // DEFAULT preset's own fix (found and patched independently of this
    // preset, out of scope for #305).
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
fn default_preset_disambiguates_colliding_query_keys() {
    let package = generate_for("swr_key_collision", TypeScriptPreset::Default);
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

fn generate_for(fixture_stem: &str, preset: TypeScriptPreset) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            preset,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("generation should succeed for this fixture/preset combination")
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
