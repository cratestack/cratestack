//! Success-path response encoding shared by `create` and `delete`.

use quote::quote;

use super::super::prep::ModelHandlerPrep;

/// Success-path response encoding shared by `create` and `delete` (each
/// of which, at this point in its handler, has a plain `result: Result<
/// Model, _>` in scope named `result` and just needs it encoded).
/// Models with **no** `@computed` field keep the pre-existing direct
/// encode, bit-identical to before this feature — see
/// `docs/design/computed-fields.md`'s "Models" section. Models **with**
/// at least one switch to the full-selection projection serializer so the
/// wire response includes resolved computed values. `update`'s tail is
/// built separately (`handlers_update.rs`) because its ETag capture must
/// read the un-projected `record` first.
pub(super) fn build_projected_response_tail(
    p: &ModelHandlerPrep,
    status: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let serialize_model_value_ident = &p.serialize_model_value_ident;

    if p.has_computed_fields {
        quote! {
            let result = match result {
                Ok(record) => #serialize_model_value_ident(
                    &state.db,
                    &state.resolvers,
                    &ctx,
                    &record,
                    &ModelSelectionQuery::default(),
                    None,
                ).await,
                Err(error) => Err(error),
            };
            ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, #status, result)
        }
    } else {
        quote! {
            ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, #status, result)
        }
    }
}
