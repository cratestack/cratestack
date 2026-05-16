use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeArity};

use crate::dart_types::dart_type;
use crate::idents::{dart_identifier, pluralize, to_camel_case, to_snake_case};
use crate::naming::{
    is_paged_model, is_relation_field, primary_key_field, procedure_wrapper_name,
    scalar_model_fields,
};
use crate::views::{
    ConstantView, ModelApiView, ProcedureView, SelectedFieldAccessorView,
    SelectedRelationAccessorView, SelectionGroupView, SelectionModelView,
};
use crate::wire_decode::decode_value_expr;

pub(crate) fn build_selection_group(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> SelectionGroupView {
    let field_group_name = format!("{}FieldNames", model.name);
    let include_group_name = format!("{}IncludeNames", model.name);
    let fields = scalar_model_fields(model, model_names)
        .into_iter()
        .map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        })
        .collect();
    let includes = model
        .fields
        .iter()
        .filter(|field| is_relation_field(model_names, field))
        .map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        })
        .collect();

    SelectionGroupView {
        field_group_name,
        fields,
        include_group_name,
        includes,
    }
}

pub(crate) fn build_selection_model(
    model: &Model,
    models: &[Model],
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> SelectionModelView {
    let scalar_fields = scalar_model_fields(model, model_names)
        .into_iter()
        .map(|field| SelectedFieldAccessorView {
            identifier: dart_identifier(&field.name),
            wire_name: field.name.clone(),
            dart_type: dart_type(&field.ty, true),
            decode_expr: decode_value_expr(
                &format!("_value['{}']", field.name),
                &field.ty,
                enum_names,
                true,
                &format!("Projected{}", model.name),
                &field.name,
            ),
        })
        .collect();
    let relations = model
        .fields
        .iter()
        .filter(|field| is_relation_field(model_names, field))
        .map(|field| {
            let target_model = models
                .iter()
                .find(|candidate| candidate.name == field.ty.name)
                .expect("validated relation should target known model");
            SelectedRelationAccessorView {
                identifier: dart_identifier(&field.name),
                wire_name: field.name.clone(),
                target_selection_class_name: format!("{}Selection", target_model.name),
                target_include_selection_class_name: format!(
                    "{}IncludeSelection",
                    target_model.name
                ),
                target_projected_class_name: format!("Projected{}", target_model.name),
                is_list: field.ty.arity == TypeArity::List,
            }
        })
        .collect();

    SelectionModelView {
        model_name: model.name.clone(),
        selection_class_name: format!("{}Selection", model.name),
        include_selection_class_name: format!("{}IncludeSelection", model.name),
        projected_class_name: format!("Projected{}", model.name),
        scalar_fields,
        relations,
    }
}

pub(crate) fn build_model_api(model: &Model) -> ModelApiView {
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    let paged = is_paged_model(model);
    ModelApiView {
        api_class_name: format!("{}Api", model.name),
        model_name: model.name.clone(),
        create_input_name: format!("Create{}Input", model.name),
        update_input_name: format!("Update{}Input", model.name),
        route: format!("/{}", pluralize(&to_snake_case(&model.name))),
        detail_route: format!("/{}/$id", pluralize(&to_snake_case(&model.name))),
        primary_key_type: dart_type(&primary_key.ty, false),
        is_paged: paged,
        list_return_type: if paged {
            format!("Page<{}>", model.name)
        } else {
            format!("List<{}>", model.name)
        },
        list_decode_expr: if paged {
            format!(
                "Page<{}>.fromWire(cratestackAsValueMap(body), decodeItem: (item) => {}.fromWire(cratestackAsValueMap(item)))",
                model.name, model.name
            )
        } else {
            format!(
                "cratestackAsValueList(body).map((item) => {}.fromWire(cratestackAsValueMap(item))).toList(growable: false)",
                model.name
            )
        },
    }
}

pub(crate) fn build_procedure(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
    enum_names: &BTreeSet<&str>,
) -> ProcedureView {
    ProcedureView {
        name: procedure.name.clone(),
        method_name: to_camel_case(&procedure.name),
        args_name: procedure_wrapper_name(procedure, occupied_type_names),
        return_type: dart_type(&procedure.return_type, false),
        route: format!("/\\$procs/{}", procedure.name),
        return_decode_expr: decode_value_expr(
            "body",
            &procedure.return_type,
            enum_names,
            false,
            "Procedure",
            &procedure.name,
        ),
        kind: match procedure.kind {
            cratestack_core::ProcedureKind::Query => "query",
            cratestack_core::ProcedureKind::Mutation => "mutation",
        },
    }
}
