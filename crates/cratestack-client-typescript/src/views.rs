use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model, Procedure, ProcedureKind, TypeArity};
use serde::Serialize;

use crate::naming::{
    escape_ts_string, pluralize, procedure_wrapper_name, to_camel_case, to_pascal_case,
    to_snake_case, ts_identifier,
};
use crate::types::{is_paged_model, primary_key_field, ts_type};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) union: String,
    pub(crate) values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InterfaceView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) property: String,
    pub(crate) wire_name: String,
    pub(crate) type_name: String,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelApiView {
    pub(crate) name: String,
    pub(crate) api_name: String,
    pub(crate) accessor: String,
    pub(crate) route: String,
    pub(crate) primary_key_type: String,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) list_return_type: String,
    pub(crate) list_query_key: String,
    pub(crate) get_query_key: String,
    pub(crate) create_mutation_key: String,
    pub(crate) update_mutation_key: String,
    pub(crate) delete_mutation_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureView {
    pub(crate) name: String,
    pub(crate) method_name: String,
    pub(crate) hook_name: String,
    pub(crate) args_name: String,
    pub(crate) return_type: String,
    pub(crate) route: String,
    pub(crate) kind: &'static str,
    pub(crate) query_key: String,
    pub(crate) mutation_key: String,
}

#[derive(Clone, Copy)]
pub(crate) enum InterfaceKind {
    Plain,
    Patch,
    Model,
}

pub(crate) fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    let values = enum_decl
        .variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    let union = values
        .iter()
        .map(|value| format!("'{}'", escape_ts_string(value)))
        .collect::<Vec<_>>()
        .join(" | ");
    EnumView {
        name: enum_decl.name.clone(),
        union,
        values,
    }
}

pub(crate) fn build_interface(
    name: &str,
    fields: &[&Field],
    kind: InterfaceKind,
    enum_names: &BTreeSet<&str>,
) -> InterfaceView {
    InterfaceView {
        name: name.to_owned(),
        has_fields: !fields.is_empty(),
        fields: fields
            .iter()
            .map(|field| {
                let optional = match kind {
                    InterfaceKind::Patch | InterfaceKind::Model => true,
                    InterfaceKind::Plain => field.ty.arity == TypeArity::Optional,
                };
                FieldView {
                    property: ts_identifier(&field.name),
                    wire_name: field.name.clone(),
                    type_name: ts_type(&field.ty, enum_names),
                    optional,
                }
            })
            .collect(),
    }
}

pub(crate) fn build_model_api(model: &Model) -> ModelApiView {
    let primary_key = primary_key_field(model).expect("validated schemas always have an id field");
    let route = format!("/{}", pluralize(&to_snake_case(&model.name)));
    let accessor = pluralize(&to_camel_case(&model.name));
    ModelApiView {
        name: model.name.clone(),
        api_name: format!("{}Api", model.name),
        accessor,
        route,
        primary_key_type: ts_type(&primary_key.ty, &BTreeSet::new()),
        create_input_name: format!("Create{}Input", model.name),
        update_input_name: format!("Update{}Input", model.name),
        list_return_type: if is_paged_model(model) {
            format!("Page<{}>", model.name)
        } else {
            format!("{}[]", model.name)
        },
        list_query_key: format!("{}List", to_camel_case(&model.name)),
        get_query_key: format!("{}Detail", to_camel_case(&model.name)),
        create_mutation_key: format!("{}Create", to_camel_case(&model.name)),
        update_mutation_key: format!("{}Update", to_camel_case(&model.name)),
        delete_mutation_key: format!("{}Delete", to_camel_case(&model.name)),
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
        hook_name: to_pascal_case(&procedure.name),
        args_name: procedure_wrapper_name(procedure, occupied_type_names),
        return_type: ts_type(&procedure.return_type, enum_names),
        route: format!("/$procs/{}", procedure.name),
        kind: match procedure.kind {
            ProcedureKind::Query => "query",
            ProcedureKind::Mutation => "mutation",
        },
        query_key: format!("{}Procedure", to_camel_case(&procedure.name)),
        mutation_key: format!("{}Procedure", to_camel_case(&procedure.name)),
    }
}

