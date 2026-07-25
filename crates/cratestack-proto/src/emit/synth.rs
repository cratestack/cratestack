//! Part B: builds the synthesized message field lists — `Create<M>Input`,
//! `Update<M>Input`, `<Procedure>Input`, `<Procedure>Output`, `PageOf<Item>`
//! (plus the one shared `PageInfo`, see `synth_page`) — as real
//! `cratestack_core::Field` values, the same shape `build_lock` already
//! knows how to number. The caller passes the result to both `build_lock`
//! (as `extra_messages`) and `emit_proto` (to render the bodies), so it's
//! built exactly once.

use std::collections::BTreeMap;

use cratestack_core::{Field, Schema};

use super::error::ProtoEmitError;
use super::mirror::{
    is_generated_on_create, is_primary_key, model_allows_create, model_name_set,
    scalar_model_fields,
};
use super::rpc_input_synth::synthesize_rpc_inputs;
use super::synth_page::{monomorphize_return_type, synthesize_pages};
use crate::casing::to_pascal_case;

pub fn synthesize_messages(
    schema: &Schema,
) -> Result<BTreeMap<String, Vec<Field>>, ProtoEmitError> {
    let model_names = model_name_set(&schema.models);
    let mut occupied: BTreeMap<String, &'static str> = BTreeMap::new();
    for model in &schema.models {
        occupied.insert(model.name.clone(), "a model");
    }
    for ty in &schema.types {
        occupied.insert(ty.name.clone(), "a type");
    }
    for enum_decl in &schema.enums {
        occupied.insert(enum_decl.name.clone(), "an enum");
    }

    let mut extra = BTreeMap::new();

    for model in &schema.models {
        let scalar_fields = scalar_model_fields(model, &model_names);
        if model_allows_create(model) {
            let fields = scalar_fields
                .iter()
                .copied()
                .filter(|field| !is_generated_on_create(field))
                .cloned()
                .collect();
            insert_synth(
                &mut occupied,
                &mut extra,
                format!("Create{}Input", model.name),
                fields,
            )?;
        }
        let update_fields = scalar_fields
            .iter()
            .copied()
            .filter(|field| !is_primary_key(field))
            .cloned()
            .collect();
        insert_synth(
            &mut occupied,
            &mut extra,
            format!("Update{}Input", model.name),
            update_fields,
        )?;
    }

    for procedure in &schema.procedures {
        let base = to_pascal_case(&procedure.name);

        let input_fields = procedure
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
        insert_synth(
            &mut occupied,
            &mut extra,
            format!("{base}Input"),
            input_fields,
        )?;

        // Every procedure gets a one-field `result` response envelope,
        // always — a deliberate uniformity choice (unlike the TS
        // generator, which doesn't need one) so ticket #170/#171's gRPC
        // method signatures don't sometimes return a bare message and
        // sometimes a wrapper depending on the return type's shape.
        let output_fields = vec![Field {
            docs: vec![],
            name: "result".to_owned(),
            name_span: procedure.name_span,
            ty: monomorphize_return_type(&procedure.return_type),
            attributes: Vec::new(),
            span: procedure.span,
        }];
        insert_synth(
            &mut occupied,
            &mut extra,
            format!("{base}Output"),
            output_fields,
        )?;
    }

    synthesize_pages(schema, &mut occupied, &mut extra)?;
    synthesize_rpc_inputs(schema, &mut occupied, &mut extra)?;

    Ok(extra)
}

pub(super) fn insert_synth(
    occupied: &mut BTreeMap<String, &'static str>,
    extra: &mut BTreeMap<String, Vec<Field>>,
    name: String,
    fields: Vec<Field>,
) -> Result<(), ProtoEmitError> {
    if let Some(conflict) = occupied.get(&name) {
        return Err(ProtoEmitError::MessageNameCollision {
            name,
            conflict: (*conflict).to_owned(),
        });
    }
    occupied.insert(name.clone(), "a synthesized message");
    extra.insert(name, fields);
    Ok(())
}
