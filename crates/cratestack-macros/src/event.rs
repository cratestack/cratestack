use cratestack_core::{Model, ModelEventKind, parse_emit_attribute};
use quote::quote;

use crate::shared::{generated_doc_attr, ident, to_snake_case};

pub(crate) fn model_emitted_events(model: &Model) -> Result<Vec<ModelEventKind>, String> {
    let mut emitted = Vec::new();
    for attribute in &model.attributes {
        if attribute.raw.starts_with("@@emit(") {
            emitted.extend(parse_emit_attribute(&attribute.raw)?);
        }
    }
    emitted.sort_by_key(|operation| match operation {
        ModelEventKind::Created => 0,
        ModelEventKind::Updated => 1,
        ModelEventKind::Deleted => 2,
    });
    emitted.dedup();
    Ok(emitted)
}

pub(crate) fn generate_event_module(models: &[Model]) -> Result<proc_macro2::TokenStream, String> {
    let aliases = models
        .iter()
        .map(generate_model_event_aliases)
        .collect::<Vec<_>>();
    let methods = models
        .iter()
        .map(generate_model_event_methods)
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        pub mod events {
            #(#aliases)*

            #[derive(Clone, Copy)]
            pub struct Subscriptions<'a> {
                runtime: &'a ::cratestack::__private::SqlxRuntime,
            }

            impl<'a> Subscriptions<'a> {
                pub(crate) fn new(runtime: &'a ::cratestack::__private::SqlxRuntime) -> Self {
                    Self { runtime }
                }

                #[doc = "Drains pending model events from the transactional outbox."]
                pub async fn drain(&self) -> Result<usize, ::cratestack::CoolError> {
                    self.runtime.drain_event_outbox().await
                }

                #(#methods)*
            }
        }
    })
}

fn generate_model_event_aliases(model: &Model) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let created_ident = ident(&format!("{}CreatedEvent", model.name));
    let updated_ident = ident(&format!("{}UpdatedEvent", model.name));
    let deleted_ident = ident(&format!("{}DeletedEvent", model.name));

    quote! {
        pub type #created_ident = ::cratestack::ModelEvent<super::models::#model_ident>;
        pub type #updated_ident = ::cratestack::ModelEvent<super::models::#model_ident>;
        pub type #deleted_ident = ::cratestack::ModelEvent<super::models::#model_ident>;
    }
}

fn generate_model_event_methods(model: &Model) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let model_ident = ident(model_name);
    let model_snake = to_snake_case(model_name);
    let methods = model_emitted_events(model)
        .map_err(|error| format!("failed to parse emitted events for `{model_name}`: {error}"))?
        .into_iter()
        .map(|operation| {
            let (method_ident, alias_ident, kind_tokens, docs) = match operation {
                ModelEventKind::Created => (
                    ident(&format!("on_{}_created", model_snake)),
                    ident(&format!("{}CreatedEvent", model_name)),
                    quote! { ::cratestack::ModelEventKind::Created },
                    generated_doc_attr(format!(
                        "Registers a subscriber for `{}` create events.",
                        model_name
                    )),
                ),
                ModelEventKind::Updated => (
                    ident(&format!("on_{}_updated", model_snake)),
                    ident(&format!("{}UpdatedEvent", model_name)),
                    quote! { ::cratestack::ModelEventKind::Updated },
                    generated_doc_attr(format!(
                        "Registers a subscriber for `{}` update events.",
                        model_name
                    )),
                ),
                ModelEventKind::Deleted => (
                    ident(&format!("on_{}_deleted", model_snake)),
                    ident(&format!("{}DeletedEvent", model_name)),
                    quote! { ::cratestack::ModelEventKind::Deleted },
                    generated_doc_attr(format!(
                        "Registers a subscriber for `{}` delete events.",
                        model_name
                    )),
                ),
            };

            quote! {
                #docs
                pub fn #method_ident<F, Fut>(&self, handler: F)
                where
                    F: Fn(#alias_ident) -> Fut + Send + Sync + 'static,
                    Fut: ::core::future::Future<Output = Result<(), ::cratestack::CoolError>>
                        + Send
                        + 'static,
                {
                    let handler = ::std::sync::Arc::new(handler);
                    self.runtime.subscribe(#model_name, #kind_tokens, move |event| {
                        let handler = ::std::sync::Arc::clone(&handler);
                        ::std::boxed::Box::pin(async move {
                            let typed = <::cratestack::ModelEvent<super::models::#model_ident> as ::core::convert::TryFrom<::cratestack::CoolEventEnvelope>>::try_from(event)?;
                            (handler)(typed).await
                        })
                    });
                }
            }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #(#methods)*
    })
}
