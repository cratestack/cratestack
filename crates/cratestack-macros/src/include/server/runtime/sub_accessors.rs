//! The two sub-accessor surfaces hanging off a `db = Postgres`
//! `Cratestack` — `views()` and `queries()` — and the modules they return
//! into.
//!
//! Split out of [`super::postgres`] (which keeps the `Cratestack`/
//! `BoundCratestack`/`CratestackBuilder` trio itself) per the workspace's
//! 200-line file ceiling. They belong together: both are read-only
//! sub-namespaces reached through one accessor method, both are
//! server-internal with no wire counterpart, and both are emitted from a
//! per-declaration token list the composer collects.
//!
//! One structural difference, and it is deliberate. `Views` holds the
//! runtime (a `ViewDelegate` is built from it directly), while `Queries`
//! holds the whole `Cratestack` — a query's generated `run` takes the
//! handle so it can reach `pool()` without the generated code needing
//! access to `SqlxRuntime`'s internals. See
//! `crate::query::accessor`'s doc comment for why the query accessor is a
//! forwarder to that `run` rather than a second entry point of its own.

use quote::quote;

/// `pub mod views { ... }` — always emitted, empty or not, because
/// `Cratestack::views()` is unconditional.
pub(super) fn views_module(
    view_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        pub mod views {
            //! View sub-accessor (ADR-0003). `runtime.views()` returns
            //! a `Views<'_>` whose methods hand out `ViewDelegate`s for
            //! each `view` block declared in the schema.
            pub struct Views<'a> {
                pub(super) runtime: &'a ::cratestack::__private::SqlxRuntime,
            }

            impl<'a> Views<'a> {
                pub(super) fn new(runtime: &'a ::cratestack::__private::SqlxRuntime) -> Self {
                    Self { runtime }
                }

                #(#view_accessors)*
            }
        }
    }
}

/// `pub fn queries(&self) -> queries::Queries<'_>` — emitted **only** when
/// the schema declares at least one `query` block (cratestack#867).
///
/// The `pub mod queries` it returns into is emitted under the same
/// condition (see `include/server.rs`), so an unconditional accessor would
/// name a module that isn't there. That module is also where the
/// `Queries` struct itself lives, rather than here beside `Views`: its
/// methods reference sibling query modules by bare path, and keeping them
/// in one module is what makes `loyalty_fee_summary::Args` resolve without
/// a `super::` prefix that would have to change if the nesting moved.
pub(super) fn queries_accessor(
    query_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    if query_accessors.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    quote! {
        /// Declarative custom-SQL reads (`query` blocks). Each method
        /// forwards to that query's generated `run`, which checks its
        /// `@allow`/`@deny` policy before executing anything.
        ///
        /// That policy gates *whether the call is permitted*, not which
        /// rows the SQL matches — a `query` body gets none of the
        /// soft-delete or row-level filtering a generated read does. See
        /// `docs/design/declarative-custom-query.md` §6.
        pub fn queries(&self) -> queries::Queries<'_> {
            queries::Queries::new(self)
        }
    }
}
