//! `db = None` runtime block (cratestack#328): a `Cratestack` type that
//! carries no `PgPool`, no `SqlxRuntime` — genuinely nothing
//! database-shaped, not an `Option<PgPool>` that happens to always be
//! `None`. There is no `.pool()` method and no way to reach a `PgPool`
//! from this type at all.
//!
//! `Cratestack` still exists (as a zero-field marker) because procedure
//! dispatch (`ProcedureRegistry` trait methods, generated in
//! `crate::procedure`) and policy evaluation (`authorize_with_db`,
//! generated in `crate::procedure::instrument`) both take `&Cratestack`
//! unconditionally — that code is shared between `db = Postgres` and
//! `db = None` on purpose (see this story's PR description for why a
//! forked dispatch path wasn't worth it: procedure policies never touch
//! `PgPool`, verified by grep across `crate::policy`). Keeping one shared
//! `&Cratestack` parameter shape means `crate::axum::procedure` and
//! `crate::procedure::instrument` don't need a `db`-conditional branch of
//! their own; only the *type behind* `Cratestack` changes.
//!
//! The one exception is a schema that declares a `topic`: it needs a
//! backplane to publish on, so its `Cratestack` carries one (an
//! `InProcessBackplane` shared by the process, unless `with_backplane` swaps
//! it) and is no longer
//! `Copy`. Every other schema keeps the zero-field marker, so adding the
//! feature broke none of them.
//!
//! `events()`/`views()` are omitted entirely rather than kept as
//! always-empty API surface: `datasource { provider = "none" }` schemas
//! can never declare a `model` (cratestack#327), so there is provably
//! nothing to subscribe to or view.

use quote::quote;

pub(super) fn build_runtime_block(topics: bool, streams: bool) -> proc_macro2::TokenStream {
    // The fields a schema's `Cratestack` carries: a backplane for topics, a
    // revocation registry for subscription procedures (cratestack#1381).
    // None of either: the zero-field `Copy` marker, as ever.
    let carries_state = topics || streams;
    let mut fields = Vec::new();
    let mut inits = Vec::new();
    let mut methods = Vec::new();
    if topics {
        fields.push(quote! {
            backplane: ::std::sync::Arc<dyn ::cratestack::SubscriptionBackplane>,
        });
        inits.push(quote! { backplane: SHARED.clone(), });
        methods.push(quote! {
            /// Replace the subscription backplane, which carries topic
            /// messages to subscribers. The default is one
            /// `InProcessBackplane` shared by every `build()` of this
            /// schema in the process: it reaches this process only, so
            /// run one replica or pass a backplane that reaches the others.
            pub fn with_backplane(
                mut self,
                backplane: ::std::sync::Arc<dyn ::cratestack::SubscriptionBackplane>,
            ) -> Self {
                self.backplane = backplane;
                self
            }
        });
    }
    if streams {
        fields.push(quote! { stream_revocations: ::cratestack::StreamRevocations, });
        inits.push(quote! { stream_revocations: Default::default(), });
        methods.push(quote! {
            /// Replace the registry live subscriptions are watched in, to
            /// bound its tombstones or name the keys streams are held
            /// under. The default never revokes anything.
            pub fn with_stream_revocations(
                mut self,
                revocations: ::cratestack::StreamRevocations,
            ) -> Self {
                self.stream_revocations = revocations;
                self
            }
        });
    }
    let shared = topics.then(|| {
        quote! {
            // One per schema and process, so a `Cratestack` built anywhere
            // (a router, a worker) reaches the same subscribers.
            static SHARED: ::std::sync::LazyLock<
                ::std::sync::Arc<::cratestack::InProcessBackplane>,
            > = ::std::sync::LazyLock::new(Default::default);
        }
    });
    let (handle, builder, new_builder, build, with_backplane, accessor) = if carries_state {
        let build_fields = builder_to_handle(topics, streams);
        (
            quote! {
                #[derive(Clone)]
                pub struct Cratestack { #(#fields)* }
            },
            quote! { pub struct CratestackBuilder { #(#fields)* } },
            quote! {
                #shared
                CratestackBuilder { #(#inits)* }
            },
            quote! { Cratestack { #build_fields } },
            quote! { #(#methods)* },
            streams.then(|| {
                quote! {
                    impl Cratestack {
                        /// The registry live subscriptions are watched in:
                        /// call `revoke` on it to end a principal's streams
                        /// (cratestack#1381).
                        pub fn stream_revocations(&self) -> &::cratestack::StreamRevocations {
                            &self.stream_revocations
                        }
                    }
                }
            }),
        )
    } else {
        (
            quote! {
                #[derive(Clone, Copy)]
                pub struct Cratestack;
            },
            quote! { pub struct CratestackBuilder; },
            quote! { CratestackBuilder },
            quote! { Cratestack },
            proc_macro2::TokenStream::new(),
            None,
        )
    };
    quote! {
        /// No-database runtime handle (`db = None`). See this module's
        /// doc comment for why it holds no pool.
        #handle

        #[derive(Clone)]
        pub struct BoundCratestack {
            ctx: ::cratestack::CratestackContext,
        }

        #builder

        #accessor

        impl Cratestack {
            pub fn builder() -> CratestackBuilder {
                #new_builder
            }

            pub fn bind_context(&self, ctx: ::cratestack::CratestackContext) -> BoundCratestack {
                BoundCratestack { ctx }
            }

            pub fn bind_auth<P: ::cratestack::serde::Serialize>(
                &self,
                principal: Option<P>,
            ) -> Result<BoundCratestack, ::cratestack::CratestackError> {
                let ctx = ::cratestack::CratestackContext::from_principal(principal)?;
                Ok(self.bind_context(ctx))
            }
        }

        impl BoundCratestack {
            pub fn context(&self) -> &::cratestack::CratestackContext {
                &self.ctx
            }
        }

        impl CratestackBuilder {
            #with_backplane

            pub fn build(self) -> Cratestack {
                #build
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
    }
}

fn builder_to_handle(topics: bool, streams: bool) -> proc_macro2::TokenStream {
    let backplane = topics.then(|| quote! { backplane: self.backplane, });
    let revocations = streams.then(|| quote! { stream_revocations: self.stream_revocations, });
    quote! { #backplane #revocations }
}
