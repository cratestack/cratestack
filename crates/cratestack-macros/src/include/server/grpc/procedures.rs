//! pb mirror structs for `transport grpc` procedures (ticket #208):
//! `<Base>Input` and `<Base>Output`, matching the message names
//! `cratestack-proto::emit::synth::synthesize_messages` already
//! synthesized into the `.pb.lock`/`.proto` (ticket #169/#170 — this
//! ticket adds zero grammar/emission changes, only the Rust runtime side
//! that was missing).
//!
//! - **`<Base>Input`** decodes an incoming request. Its fields are
//!   exactly `procedure.args`, in the same order
//!   `cratestack-proto::emit::synth` used to number them, and the domain
//!   struct is the procedure module's own `Args` (`crate::procedure::
//!   generate_procedure_args_struct`) — same field names, same order —
//!   so [`crate::include::grpc_pb::message::render_message`] (already
//!   used for `Create<M>Input`/`Update<M>Input`) applies unchanged.
//! - **`<Base>Output`** encodes the response. Unlike every other message
//!   in this crate it is never decoded server-side (a procedure's output
//!   only ever flows server -> client), so it gets a `From<&Output>`
//!   impl only, no `TryFrom`. `Output` wraps the procedure's return
//!   value in one synthetic `result` field
//!   (`cratestack_proto::monomorphize_return_type`, the same `Page<T>`
//!   -> `PageOf<Item>` monomorphization `synthesize_messages` already
//!   applied when it numbered this field) — reusing
//!   [`crate::include::grpc_pb::message::render_field`] for that single
//!   field's scalar/enum/message + arity handling rather than
//!   re-deriving it, passing `(*value).clone()` as the domain expression
//!   since there is no struct field to project through here — the
//!   domain value the wire field wraps *is* `value` itself.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Procedure};
use quote::quote;

use crate::include::grpc_pb::message::{self, render_message};
use crate::shared::{ident, to_snake_case};

/// `<Base>Input { <arg wire fields...> }` plus `TryFrom<Input> for
/// procedures::<name>::Args` — the direction the unary/streaming service
/// arms in [`super::service`] actually call.
pub(super) fn render_procedure_input(
    procedure: &Procedure,
    numbers: &BTreeMap<String, i32>,
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let base = cratestack_proto::to_pascal_case(&procedure.name);
    let message_name = format!("{base}Input");
    let module_ident = ident(&to_snake_case(&procedure.name));
    let domain_path = quote! { super::super::procedures::#module_ident::Args };

    let fields: Vec<Field> = procedure
        .args
        .iter()
        .map(|arg| Field {
            docs: arg.docs.clone(),
            name: arg.name.clone(),
            name_span: arg.name_span,
            ty: arg.ty.clone(),
            attributes: Vec::new(),
            span: arg.span,
        })
        .collect();
    let field_refs: Vec<&Field> = fields.iter().collect();

    let rendered = render_message(&message_name, domain_path, &field_refs, numbers, enum_names)?;
    Ok(rendered.tokens)
}

/// `<Base>Output { optional/repeated <wire result type> result = N; }`
/// plus `From<&procedures::<name>::Output>` only (see module doc for
/// why no `TryFrom`).
pub(super) fn render_procedure_output(
    procedure: &Procedure,
    numbers: &BTreeMap<String, i32>,
    enum_names: &BTreeSet<&str>,
) -> Result<proc_macro2::TokenStream, String> {
    let base = cratestack_proto::to_pascal_case(&procedure.name);
    let message_name = format!("{base}Output");
    let ident_tok = ident(&message_name);
    let module_ident = ident(&to_snake_case(&procedure.name));
    let domain_path = quote! { super::super::procedures::#module_ident::Output };

    let number = *numbers
        .get("result")
        .ok_or_else(|| format!("no `.pb.lock` entry for `{message_name}.result`"))?;
    let result_ty = cratestack_proto::monomorphize_return_type(&procedure.return_type);
    let domain_expr = quote! { (*value).clone() };
    let plan = message::render_field(
        &message_name,
        "result",
        &result_ty,
        domain_expr,
        number,
        enum_names,
    );
    let prost_field = plan.prost_field;
    let from_domain_init = plan.from_domain_init;

    Ok(quote! {
        #[derive(Clone, PartialEq, ::cratestack::grpc::prost::Message)]
        pub struct #ident_tok {
            #prost_field
        }

        impl ::core::convert::From<&#domain_path> for #ident_tok {
            fn from(value: &#domain_path) -> Self {
                Self {
                    #from_domain_init
                }
            }
        }
    })
}
