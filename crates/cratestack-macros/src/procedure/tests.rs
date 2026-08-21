//! Regression guard for cratestack#282: a `T[]`-returning procedure with
//! no `@stream` attribute must generate the exact same
//! `ProcedureRegistry` trait method today generates — byte-identical, not
//! just "equivalent". `non_stream_list_procedure_trait_method_is_unchanged`
//! below was written and confirmed passing against the pre-`@stream`
//! implementation *before* the `@stream` branch in
//! `generate_procedure_registry_method` was added — its assertion has not
//! moved since, so it still passing is the "diff of zero lines" proof
//! cratestack#282's acceptance criteria ask for.
//!
//! `cratestack-macros` is a `proc-macro = true` crate but that only
//! restricts what it can export as a public proc-macro entry point —
//! ordinary `#[test]` functions calling its own `pub(crate)` codegen
//! functions and asserting on `TokenStream::to_string()` work exactly like
//! in any other crate, and (per `cratestack-macros` having no `tests/`
//! directory at all) is the established way to unit test codegen here.

use quote::quote;

use super::generate_procedure_registry_method;
use super::instrument::invoke_with_db_fn_tokens;

/// A minimal, realistic schema mirroring `examples/rpc-streaming`'s
/// `ticks` procedure — a `Tick[]`-returning query procedure.
const LIST_RETURNING_SCHEMA: &str = r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
"#;

const STREAM_SCHEMA: &str = r#"
type TickerArgs {
  symbol String
}

type Tick {
  price Float
}

procedure ticks(args: TickerArgs): Tick[]
  @stream
"#;

fn parse_first_procedure(source: &str) -> cratestack_core::Procedure {
    cratestack_parser::parse_schema(source)
        .expect("fixture schema should parse and validate")
        .procedures
        .remove(0)
}

/// This is the pinned baseline: the exact trait method
/// `generate_procedure_registry_method` emitted before `@stream` existed,
/// for a `Tick[]` procedure with no `@stream` attribute. Do not "fix" this
/// string when touching `@stream` codegen — if this test fails after that
/// change, the change broke the non-breaking guarantee cratestack#282
/// requires.
///
/// **Deliberately re-pinned once, for cratestack#512.** Both branches below
/// gained a trailing `_authorized: ticks::Authorized` parameter — a real,
/// intentional, documented-breaking change (every `ProcedureRegistry`
/// implementor's method signature grows this parameter; see
/// `generate_procedure_registry_method`'s own doc comment and the
/// CHANGELOG's Migration paragraph), unrelated to cratestack#282's
/// stream-vs-non-stream parity this test actually guards. Both branches
/// were re-pinned together specifically so that parity — "adding
/// `@stream` doesn't change anything else about the trait method" — still
/// holds after the cratestack#512 change, which is why both quote! blocks
/// below gained the identical new parameter rather than just one of them.
#[test]
fn non_stream_list_procedure_trait_method_is_unchanged() {
    let procedure = parse_first_procedure(LIST_RETURNING_SCHEMA);

    let actual = generate_procedure_registry_method(&procedure)
        .expect("codegen should succeed")
        .to_string();

    let expected = quote! {
        fn ticks(
            &self,
            db: &super::Cratestack,
            ctx: &::cratestack::CratestackContext,
            args: ticks::Args,
            _authorized: ticks::Authorized,
        ) -> impl ::core::future::Future<Output = Result<ticks::Output, ::cratestack::CratestackError>> + Send;
    }
    .to_string();

    assert_eq!(
        actual, expected,
        "non-@stream list-returning procedure's trait method must stay byte-identical \
         (modulo the cratestack#512 witness parameter, re-pinned above)"
    );
}

#[test]
fn stream_marked_list_procedure_generates_stream_shaped_trait_method() {
    let procedure = parse_first_procedure(STREAM_SCHEMA);

    let actual = generate_procedure_registry_method(&procedure)
        .expect("codegen should succeed")
        .to_string();

    let expected = quote! {
        fn ticks(
            &self,
            db: &super::Cratestack,
            ctx: &::cratestack::CratestackContext,
            args: ticks::Args,
            _authorized: ticks::Authorized,
        ) -> impl ::cratestack::futures::Stream<Item = Result<ticks::Item, ::cratestack::CratestackError>> + Send;
    }
    .to_string();

    assert_eq!(
        actual, expected,
        "@stream-marked procedure must generate a Stream-shaped trait method, not Future<Output = Vec<T>>"
    );
}

/// Every `#[doc = "..."]` string literal on the parsed item, in source
/// order — i.e. one entry per original `///` line, exactly as rustdoc's
/// own CommonMark scanner would see them once reassembled.
///
/// Deliberately walks `syn::Attribute` rather than substring-matching
/// `TokenStream::to_string()` directly: `to_string()` flattens every
/// `#[doc = "..."]` onto one line with no line boundaries preserved, so a
/// naive `.contains("```ignore")` also fires on this test file's own
/// explanatory prose about `` ```ignore `` (which necessarily contains
/// that substring to talk about it) once such prose is embedded in the
/// generated doc comment itself — a false positive, not a real fence.
/// Preserving line boundaries is what makes the fence-open check below
/// precise instead of a substring guess.
fn doc_lines(tokens: proc_macro2::TokenStream) -> Vec<String> {
    let item: syn::ItemFn = syn::parse2(tokens).expect("invoke_with_db tokens must parse as a fn");
    item.attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(syn::MetaNameValue {
                value:
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(literal),
                        ..
                    }),
                ..
            }) => Some(literal.value()),
            _ => None,
        })
        .collect()
}

/// True if `line` opens a markdown fence rustdoc schedules as a doctest
/// with `attribute` in its info string (`ignore`, `no_run`,
/// `compile_fail`, ...). Mirrors CommonMark's actual fence rule closely
/// enough for this generated, hand-authored doc comment: the delimiter
/// run must be *exactly* three backticks — a run of four or more (as this
/// very file's own explanatory prose uses, to talk about a fence without
/// opening one) is a longer/different fence marker, not this one, and a
/// bare `` ``` `` with no info string at all defaults to Rust with no
/// `ignore`/`no_run`/`compile_fail` attribute, so it isn't this collision
/// either.
fn opens_fence_with_attribute(line: &str, attribute: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return false;
    };
    if rest.starts_with('`') {
        return false; // four-or-more-backtick run: a different fence, not this one.
    }
    rest.split(|c: char| c == ',' || c.is_whitespace())
        .any(|word| word == attribute)
}

/// Regression guard for cratestack#611: `invoke_with_db`'s generated doc
/// comment used to carry its illustrative example fenced ```` ```ignore ````.
/// `cargo test --doc` (no flags) correctly skips a ```` ```ignore ````
/// doctest, but `cargo test --doc -- --ignored` reuses the exact same
/// "ignored" bucket to force-compile it — and the example was never meant
/// to compile (it references a nonexistent `reconcile_accounts` procedure
/// and free variables `db`/`registry`/`ctx` with no scope to resolve
/// against). Every downstream schema with at least one procedure got one
/// guaranteed failure per procedure the moment its own CI included
/// `-- --ignored` anywhere.
///
/// This test operates directly on the `TokenStream` `invoke_with_db_fn_tokens`
/// produces — the actual generated-code artifact, one level more direct
/// than parsing a full schema through `generate_procedure_module` — since
/// `quote!`'s `///` sugar lowers to `#[doc = "..."]` attributes carrying
/// the literal string content, so the fenced fragment inside the doc string
/// survives intact and is exactly what rustdoc would later scan for
/// doctest candidates. This is more direct than expanding an entire schema
/// through the macro entry point, and avoids needing a fixture crate +
/// `cargo test --doc -- --ignored` subprocess just to prove the same fact
/// about one generated function's doc comment.
///
/// Without the fix this FAILS: the pre-fix source contained a literal
/// ` ```ignore ` fence (no other language annotation) that rustdoc's
/// CommonMark doctest scanner treats as "skip normally, force-run under
/// `--ignored`" — exactly the collision cratestack#611 reports.
#[test]
fn invoke_with_db_doc_comment_has_no_ignore_fenced_doctest() {
    let lines = doc_lines(invoke_with_db_fn_tokens());

    for attribute in ["ignore", "no_run", "compile_fail"] {
        assert!(
            !lines
                .iter()
                .any(|line| opens_fence_with_attribute(line, attribute)),
            "invoke_with_db's generated doc comment must not open a \
             ```{attribute} fenced block: `cargo test -- --ignored` \
             force-compiles ```ignore``` doctests (and no_run/compile_fail \
             ones always compile), and the illustrative example was never \
             meant to compile (cratestack#611). Doc lines:\n{lines:#?}"
        );
    }
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("```text")),
        "the illustrative example should still render as a fenced code \
         block in `cargo doc` output — just under a language rustdoc \
         never treats as a doctest candidate, so the documentation value \
         from cratestack#512 is preserved. Doc lines:\n{lines:#?}"
    );
}
