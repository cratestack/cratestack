//! `db = Postgres` runtime block: `Cratestack` (the sqlx-backed delegate
//! hub), `BoundCratestack` (context-bound view), `CratestackBuilder`, plus
//! `schema_summary()`. Byte-identical to the pre-cratestack#328 output —
//! this file is the exact original `build_runtime_block` body, just moved
//! under `runtime/` alongside its `db = None` sibling
//! ([`super::none::build_runtime_block`]).

use quote::quote;

pub(super) fn build_runtime_block(
    model_accessors: &[proc_macro2::TokenStream],
    bound_model_accessors: &[proc_macro2::TokenStream],
    view_accessors: &[proc_macro2::TokenStream],
    query_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    let views_module = quote! {
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
    };

    // `queries()` only exists for a schema that declares at least one
    // `query` block — the `pub mod queries` it returns into is emitted
    // under the same condition (see `include/server.rs`), so emitting the
    // accessor unconditionally would name a module that isn't there.
    let queries_accessor = if query_accessors.is_empty() {
        proc_macro2::TokenStream::new()
    } else {
        quote! {
            /// Declarative custom-SQL reads (`query` blocks). Each method
            /// forwards to that query's generated `run`, which checks its
            /// `@allow`/`@deny` policy before executing anything.
            pub fn queries(&self) -> queries::Queries<'_> {
                queries::Queries::new(self)
            }
        }
    };

    quote! {
        #[derive(Clone)]
        pub struct Cratestack {
            runtime: ::cratestack::__private::SqlxRuntime,
        }

        #[derive(Clone)]
        pub struct BoundCratestack<'a> {
            inner: &'a Cratestack,
            ctx: ::cratestack::CratestackContext,
        }

        pub struct CratestackBuilder {
            runtime: ::cratestack::__private::SqlxRuntime,
        }

        impl Cratestack {
            pub fn builder(pool: ::cratestack::sqlx::PgPool) -> CratestackBuilder {
                CratestackBuilder {
                    runtime: ::cratestack::__private::SqlxRuntime::new(pool),
                }
            }

            pub fn bind_context(&self, ctx: ::cratestack::CratestackContext) -> BoundCratestack<'_> {
                BoundCratestack { inner: self, ctx }
            }

            pub fn pool(&self) -> &::cratestack::sqlx::PgPool {
                self.runtime.pool()
            }

            /// Compose several writes in one Postgres transaction without
            /// naming a `sqlx` type (cratestack#513): `body` receives an
            /// opaque `::cratestack::Tx` it can pass straight to any write
            /// builder's `run_in_tx` (e.g.
            /// `self.some_model().create(..).run_in_tx(tx, &ctx)`); commits
            /// on `Ok`, rolls back on `Err`. See
            /// `::cratestack::__private::SqlxRuntime::transaction`'s doc
            /// comment for the full rollback-timing and no-retry rationale.
            ///
            /// **Composing `run_in_tx` calls through here does not, by
            /// itself, fan out to an installed `AuditSink` or deliver
            /// `@@emit` events (cratestack#534)** — that stays exactly as
            /// caller-driven as it is when you open the transaction
            /// yourself via `self.pool().begin()`. Collect each
            /// `RunInTxOutcome::audit_events` and call
            /// [`Cratestack::dispatch_audit_sink`] once this returns `Ok`,
            /// and call `self.events().drain()` for the outbox half. See
            /// `::cratestack::__private::SqlxRuntime::transaction`'s doc
            /// comment for why this can't be made automatic here.
            pub async fn transaction<F, T>(
                &self,
                body: F,
            ) -> Result<T, ::cratestack::CratestackError>
            where
                F: AsyncFnOnce(&mut ::cratestack::Tx) -> Result<T, ::cratestack::CratestackError>,
            {
                self.runtime.transaction(body).await
            }

            /// Fan out `AuditEvent`s a caller-managed `run_in_tx`
            /// transaction built and persisted, but could not dispatch
            /// itself (cratestack#534): `run_in_tx` returns a
            /// `RunInTxOutcome` carrying the events instead of dispatching
            /// them, because it has no reliable "after commit" point of
            /// its own — the caller owns `tx` and decides if/when it
            /// commits. Call this once, after your own `tx.commit()`
            /// succeeds, passing the concatenated `audit_events` from
            /// every `RunInTxOutcome` produced inside that transaction.
            /// Errors are logged, not returned — same best-effort contract
            /// as the dispatch `.run(..)` already performs automatically;
            /// see `::cratestack::__private::dispatch_audit_sink`'s doc
            /// comment for the full reasoning.
            ///
            /// Calling this is the caller's responsibility, not something
            /// `run_in_tx` or this combinator does for you — see
            /// cratestack#534's PR body for why option (a) (this) was
            /// chosen over a runtime-owned commit hook (option (b)) or
            /// leaving the gap undocumented (option (c)).
            ///
            /// The `@@emit` event outbox has the identical after-commit
            /// gap, but does **not** need an equivalent new method here:
            /// `enqueue_event_outbox` already writes its row inside `tx`
            /// (same in-transaction guarantee this crate gives the audit
            /// table), and draining re-scans `cratestack_event_outbox` for
            /// anything undelivered rather than needing a specific event
            /// handed back — so the pre-existing `Cratestack::events().drain()`
            /// (cratestack#390) already closes that half; call it the same
            /// way, after your own commit succeeds.
            pub async fn dispatch_audit_sink(&self, events: &[::cratestack::AuditEvent]) {
                ::cratestack::__private::dispatch_audit_sink(&self.runtime, events).await
            }

            pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                &self,
                principal: Option<P>,
            ) -> Result<BoundCratestack<'_>, ::cratestack::CratestackError> {
                let ctx = ::cratestack::CratestackContext::from_principal(principal)?;
                Ok(self.bind_context(ctx))
            }

            #(#model_accessors)*

            pub fn events(&self) -> events::Subscriptions<'_> {
                events::Subscriptions::new(&self.runtime)
            }

            pub fn views(&self) -> views::Views<'_> {
                views::Views::new(&self.runtime)
            }

            #queries_accessor
        }

        impl<'a> BoundCratestack<'a> {
            pub fn context(&self) -> &::cratestack::CratestackContext {
                &self.ctx
            }

            /// See [`Cratestack::dispatch_audit_sink`] — identical
            /// contract, just forwarded through the bound handle so
            /// `.bind(ctx)` callers don't need to hold onto the unbound
            /// `Cratestack` separately.
            pub async fn dispatch_audit_sink(&self, events: &[::cratestack::AuditEvent]) {
                self.inner.dispatch_audit_sink(events).await
            }

            #(#bound_model_accessors)*
        }

        impl CratestackBuilder {
            /// Install a custom [`::cratestack::AuditSink`] (cratestack#473) —
            /// every `@@audit` mutation run through the built `Cratestack`
            /// fans out to it, in addition to the always-on
            /// `cratestack_audit` table row. See
            /// `::cratestack::__private::SqlxRuntime::with_audit_sink`'s doc
            /// comment for the in-transaction-vs-post-commit dispatch
            /// contract.
            pub fn with_audit_sink(
                mut self,
                sink: ::std::sync::Arc<dyn ::cratestack::AuditSink>,
            ) -> Self {
                self.runtime = self.runtime.with_audit_sink(sink);
                self
            }

            pub fn build(self) -> Cratestack {
                Cratestack {
                    runtime: self.runtime,
                }
            }
        }

        pub fn schema_summary() -> ::cratestack::SchemaSummary {
            ::cratestack::SchemaSummary {
                mixins: MIXINS,
                models: MODELS,
                types: TYPES,
                enums: ENUMS,
                procedures: PROCEDURES,
                views: VIEWS,
            }
        }

        #views_module
    }
}
