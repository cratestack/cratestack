//! The schema's `topics` module (`docs/design/subscriptions-websocket.md`
//! §3.4, slice S4): `db.topics().room_message(&room_id)` returns a typed
//! `cratestack::Topic<ChatMessage>` on the runtime's subscription backplane,
//! to `publish`, `publish_in_tx` or `subscribe`.
//!
//! A topic is server-internal: no op, no route, no policy. Only `db =
//! Postgres` has a backplane to carry it, so a topic under `db = None` is a
//! compile error rather than a handle with nowhere to publish.

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
) -> Result<proc_macro2::TokenStream, String> {
    if topics.is_empty() {
        return Ok(proc_macro2::TokenStream::new());
    }
    if !postgres {
        return Err(format!(
            "topic `{}` needs a subscription backplane, and `db = None` has none yet: \
             return the subscription procedure's stream from your own source instead",
            topics[0].name,
        ));
    }
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
                ::cratestack::Topic::new(self.backplane.clone(), #module::NAME, #key)
            }
        }
    });
    Ok(quote! {
        /// The schema's `topic` declarations: typed messages carried to
        /// every replica by the subscription backplane.
        pub mod topics {
            #(#aliases)*

            /// One method per `topic`. Get it with `Cratestack::topics()`.
            #[derive(Clone)]
            pub struct Topics {
                backplane: ::std::sync::Arc<dyn ::cratestack::SubscriptionBackplane>,
            }

            impl Topics {
                #(#accessors)*
            }

            impl super::Cratestack {
                /// This schema's topics, on the runtime's backplane.
                pub fn topics(&self) -> Topics {
                    Topics {
                        backplane: self.runtime.backplane().clone(),
                    }
                }
            }
        }
    })
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

    fn generate(postgres: bool) -> Result<String, String> {
        let schema = cratestack_parser::parse_schema(SCHEMA).expect("parses");
        super::generate_topics_module(&schema.topics, &schema.types, &BTreeSet::new(), postgres)
            .map(|tokens| tokens.to_string())
    }

    #[test]
    fn a_topic_gets_a_typed_accessor_on_the_backplane() {
        let code = generate(true).unwrap();
        assert!(code.contains("pub fn room (& self , room_id : & str) -> :: cratestack :: Topic < room :: Payload >"), "{code}");
        assert!(code.contains("self . runtime . backplane ()"));
    }

    #[test]
    fn a_topic_without_a_database_is_an_error() {
        let error = generate(false).unwrap_err();
        assert!(error.contains("`db = None` has none yet"), "{error}");
    }
}
