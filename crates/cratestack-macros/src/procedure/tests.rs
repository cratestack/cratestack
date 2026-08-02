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
            ctx: &::cratestack::CoolContext,
            args: ticks::Args,
        ) -> impl ::core::future::Future<Output = Result<ticks::Output, ::cratestack::CoolError>> + Send;
    }
    .to_string();

    assert_eq!(
        actual, expected,
        "non-@stream list-returning procedure's trait method must stay byte-identical"
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
            ctx: &::cratestack::CoolContext,
            args: ticks::Args,
        ) -> impl ::cratestack::futures::Stream<Item = Result<ticks::Item, ::cratestack::CoolError>> + Send;
    }
    .to_string();

    assert_eq!(
        actual, expected,
        "@stream-marked procedure must generate a Stream-shaped trait method, not Future<Output = Vec<T>>"
    );
}
