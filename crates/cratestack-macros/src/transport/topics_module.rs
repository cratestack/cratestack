//! The schema's `topics` module (`docs/design/subscriptions-websocket.md`
//! §3.4, slice S4): `db.topics().room_message(&room_id)` returns a typed
//! `cratestack::Topic<ChatMessage>` on the runtime's subscription backplane,
//! to `publish`, `publish_in_tx` or `subscribe`.
//!
//! A topic is server-internal: no op, no route, no policy. Under `db =
//! Postgres` the handle is `cratestack-pg`'s `Topic` (with `publish_in_tx`),
//! under `db = None` the backplane-only one from `cratestack-api`; the
//! generated code is the same either way, only where the backplane lives
//! differs.

use std::collections::BTreeSet;

use cratestack_core::{Topic, TypeDecl};
use quote::quote;

use crate::procedure::procedure_output_tokens;
use crate::shared::{doc_attrs, ident, to_snake_case};

pub(crate) fn generate_topics_module(
    topics: &[Topic],
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
    postgres: bool,
) -> proc_macro2::TokenStream {
    if topics.is_empty() {
        return proc_macro2::TokenStream::new();
    }
    // Only `db = Postgres` has `@isolation` attempts to bind a topic to
    // (`IsolatedCratestack::topics()`); `db = None` has no sqlx at all.
    let (backplane, attempt_field, attempt_ctor, attempt_init, bind) = if postgres {
        (
            quote! { self.runtime.backplane().clone() },
            quote! {
                /// Set only by `IsolatedCratestack::topics()`.
                attempt: Option<::cratestack::__private::SqlxRuntime>,
            },
            quote! {
                /// The topics of an `@isolation` procedure's attempt.
                pub(super) fn in_attempt(runtime: ::cratestack::__private::SqlxRuntime) -> Self {
                    Self {
                        backplane: runtime.backplane().clone(),
                        attempt: Some(runtime),
                    }
                }
            },
            quote! { attempt: None, },
            quote! { .in_attempt(self.attempt.as_ref()) },
        )
    } else {
        let none = proc_macro2::TokenStream::new;
        (
            quote! { self.backplane.clone() },
            none(),
            none(),
            none(),
            none(),
        )
    };
    let aliases = topics.iter().map(|topic| {
        let module = ident(&to_snake_case(&topic.name));
        // Nested one module deeper, at a procedure module's depth, so the
        // payload type resolves from the same path tokens.
        let payload = procedure_output_tokens(&topic.payload, types, enum_names);
        let name = &topic.name;
        quote! {
            pub mod #module {
                pub const NAME: &str = #name;
                pub type Payload = #payload;
            }
        }
    });
    let accessors = topics.iter().map(|topic| {
        let module = ident(&to_snake_case(&topic.name));
        let key = ident(&to_snake_case(&topic.key.name));
        let key_ty = match topic.key.ty.name.as_str() {
            "Int" => quote! { i64 },
            "Uuid" => quote! { ::cratestack::uuid::Uuid },
            _ => quote! { &str },
        };
        let docs = doc_attrs(&topic.docs);
        quote! {
            #docs
            pub fn #module(&self, #key: #key_ty) -> ::cratestack::Topic<#module::Payload> {
                ::cratestack::Topic::new(self.backplane.clone(), #module::NAME, #key) #bind
            }
        }
    });
    quote! {
        /// The schema's `topic` declarations: typed messages carried to
        /// every replica by the subscription backplane.
        pub mod topics {
            #(#aliases)*

            /// One method per `topic`. Get it with `Cratestack::topics()`.
            #[derive(Clone)]
            pub struct Topics {
                backplane: ::std::sync::Arc<dyn ::cratestack::SubscriptionBackplane>,
                #attempt_field
            }

            impl Topics {
                #attempt_ctor

                #(#accessors)*
            }

            impl super::Cratestack {
                /// This schema's topics, on the runtime's backplane.
                pub fn topics(&self) -> Topics {
                    Topics {
                        backplane: #backplane,
                        #attempt_init
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    const SCHEMA: &str = r#"
type Message {
  body String
}

topic Room(roomId: String): Message
"#;

    fn generate(postgres: bool) -> String {
        let schema = cratestack_parser::parse_schema(SCHEMA).expect("parses");
        super::generate_topics_module(&schema.topics, &schema.types, &BTreeSet::new(), postgres)
            .to_string()
    }

    #[test]
    fn a_topic_gets_a_typed_accessor_on_the_backplane() {
        let code = generate(true);
        assert!(code.contains("pub fn room (& self , room_id : & str) -> :: cratestack :: Topic < room :: Payload >"), "{code}");
        assert!(code.contains("self . runtime . backplane ()"));
        assert!(
            code.contains(". in_attempt (self . attempt . as_ref ())"),
            "{code}"
        );
    }

    #[test]
    fn a_topic_without_a_database_reads_the_runtimes_own_backplane() {
        let code = generate(false);
        assert!(
            code.contains("backplane : self . backplane . clone ()"),
            "{code}"
        );
        assert!(!code.contains("self . runtime"), "{code}");
        assert!(!code.contains("attempt"), "{code}");
    }
}
