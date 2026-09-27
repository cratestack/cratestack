//! `IsolatedCratestack` — the database handle an `@isolation` procedure's
//! `ProcedureRegistry` method receives — and the `db = None` guard that
//! refuses `@isolation` where there is no database
//! (docs/design/procedure-isolation.md §3, §8; GHSA-r67q-4qqq-g9gm).
//!
//! Emitted only for `db = Postgres` schemas that declare at least one
//! `@isolation` procedure, so every other schema's expansion is unchanged.

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use super::super::parse::ServerDb;
use crate::model::generate_isolated_model_accessor;
use crate::shared::procedure_isolation;

fn isolated_procedures(schema: &cratestack_core::Schema) -> Vec<&str> {
    schema
        .procedures
        .iter()
        .filter(|procedure| procedure_isolation(procedure).is_some())
        .map(|procedure| procedure.name.as_str())
        .collect()
}

/// `db = None` without a `datasource` block reaches codegen even though the
/// parser refuses `@isolation` under `provider = "none"` — the same gap
/// `query_guard` closes for `query` blocks.
pub(super) fn guard_no_isolation_without_a_database(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
    db: ServerDb,
) -> Result<(), TokenStream> {
    let names = isolated_procedures(schema);
    if db != ServerDb::None || names.is_empty() {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "procedure(s) {} declare `@isolation`, but this macro call says `db = None`, \
                 which configures no database — there is no transaction to isolate. Remove \
                 `@isolation`, or switch this call to `db = Postgres`. See \
                 docs/design/procedure-isolation.md §8.",
                names.join(", "),
            ),
        )
        .to_compile_error(),
    ))
}

/// The `ComputedFieldResolver` trait's doc when the schema declares an
/// `@isolation` procedure: its resolvers can run inside that procedure's
/// attempt, and re-run with it (docs/design/procedure-isolation.md §6).
/// Empty otherwise, so every other schema's expansion is unchanged.
pub(super) fn computed_resolver_doc_tokens(
    schema: &cratestack_core::Schema,
    db: ServerDb,
) -> proc_macro2::TokenStream {
    if db != ServerDb::Postgres || isolated_procedures(schema).is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let doc = " Resolves `@computed` fields. For the output of an `@isolation` procedure \
               (this schema declares at least one), the resolvers run inside that \
               procedure's transaction attempt, after its body and before `COMMIT`: the \
               `&Cratestack` they receive is bound to the attempt, a resolver error rolls \
               the attempt back, and on a serialization failure or deadlock \
               (`40001`/`40P01`) the whole attempt — body and resolvers — runs again. A \
               resolver must therefore be re-runnable: what it does through `db`'s model \
               accessors and `transaction(..)` is rolled back with a failed attempt, but \
               anything else (HTTP calls, e-mail, state in `self`, and `db.pool()`, \
               `views()`, `queries()` and `events()`, which run on the pool) is repeated. \
               Calling another `@isolation` procedure's `invoke_with_db` with that `db` \
               joins the same attempt (a savepoint, never its own transaction) and is \
               refused if it declares a stricter level; such joined calls must run one at \
               a time, and starting one while another is still running fails the attempt. See \
               docs/design/procedure-isolation.md.";
    quote! { #[doc = #doc] }
}

pub(super) fn isolated_handle_tokens(
    schema: &cratestack_core::Schema,
    db: ServerDb,
) -> proc_macro2::TokenStream {
    if db != ServerDb::Postgres || isolated_procedures(schema).is_empty() {
        return proc_macro2::TokenStream::new();
    }
    let accessors = schema.models.iter().map(generate_isolated_model_accessor);
    quote! {
        /// The database handle of an `@isolation` procedure: every
        /// operation made through it runs inside the procedure's own
        /// transaction, at the declared level, and is rolled back with an
        /// attempt that hits a serialization failure and is retried.
        ///
        /// Unlike [`Cratestack`] it has no `pool()` — nothing reachable
        /// from it runs outside that transaction — and no `events()`,
        /// `views()` or `queries()`, which execute on the pool. Raw SQL goes
        /// through [`IsolatedCratestack::transaction`]. It cannot be built
        /// outside this generated module. See
        /// docs/design/procedure-isolation.md.
        pub struct IsolatedCratestack {
            inner: Cratestack,
        }

        impl IsolatedCratestack {
            #(#accessors)*

            /// A context-bound view over the same transaction.
            pub fn bind_context(&self, ctx: ::cratestack::CratestackContext) -> BoundCratestack<'_> {
                self.inner.bind_context(ctx)
            }

            /// See [`Cratestack::bind_auth`]; bound to the same transaction.
            pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                &self,
                principal: Option<P>,
            ) -> Result<BoundCratestack<'_>, ::cratestack::CratestackError> {
                self.inner.bind_auth(principal)
            }

            /// A savepoint inside the procedure's transaction: released when
            /// `body` returns `Ok`, rolled back to when it returns `Err`.
            /// `tx` is the isolated transaction itself — pass it to any
            /// write builder's `run_in_tx(tx, ctx)`, or run raw SQL on
            /// `&mut ***tx`. While `body` runs, the handle's own `.run(ctx)`
            /// calls are refused (one statement at a time on one
            /// connection); use `run_in_tx` inside instead.
            pub async fn transaction<F, T>(&self, body: F) -> Result<T, ::cratestack::CratestackError>
            where
                F: AsyncFnOnce(&mut ::cratestack::Tx) -> Result<T, ::cratestack::CratestackError>,
            {
                self.inner.transaction(body).await
            }

            /// Queue `AuditEvent`s from `run_in_tx` outcomes for the
            /// installed `AuditSink`; they are dispatched once, after the
            /// procedure's transaction commits, and dropped if it does not.
            pub async fn dispatch_audit_sink(&self, events: &[::cratestack::AuditEvent]) {
                self.inner.dispatch_audit_sink(events).await
            }
        }
    }
}
