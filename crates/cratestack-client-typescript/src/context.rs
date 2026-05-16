use cratestack_core::{Field, Schema};
use serde::Serialize;

use crate::config::TypeScriptGeneratorConfig;
use crate::naming::{occupied_type_names, package_class_stem, to_pascal_case};
use crate::types::{enum_name_set, is_generated_on_create, is_primary_key, model_name_set, scalar_model_fields};
use crate::views::{
    build_enum_view, build_interface, build_model_api, build_procedure, EnumView, InterfaceKind,
    InterfaceView, ModelApiView, ProcedureView,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemplateContext {
    package_name: String,
    client_class_name: String,
    base_path: String,
    enums: Vec<EnumView>,
    interfaces: Vec<InterfaceView>,
    models: Vec<ModelApiView>,
    procedures: Vec<ProcedureView>,
    query_procedures: Vec<ProcedureView>,
    mutation_procedures: Vec<ProcedureView>,
}

pub(crate) fn build_template_context(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> TemplateContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied_type_names = occupied_type_names(schema);
    let client_class_name = format!(
        "{}Client",
        to_pascal_case(&package_class_stem(&config.package_name))
    );

    let enums = schema.enums.iter().map(build_enum_view).collect();
    let mut interfaces = Vec::new();
    for ty in &schema.types {
        interfaces.push(build_interface(
            &ty.name,
            &ty.fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }
    for model in &schema.models {
        let scalar_fields = scalar_model_fields(model, &model_names);
        interfaces.push(build_interface(
            &model.name,
            &model.fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Model,
            &enum_names,
        ));
        interfaces.push(build_interface(
            &format!("Create{}Input", model.name),
            &scalar_fields
                .iter()
                .copied()
                .filter(|field| !is_generated_on_create(field))
                .collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
        interfaces.push(build_interface(
            &format!("Update{}Input", model.name),
            &scalar_fields
                .iter()
                .copied()
                .filter(|field| !is_primary_key(field))
                .collect::<Vec<_>>(),
            InterfaceKind::Patch,
            &enum_names,
        ));
    }
    for procedure in &schema.procedures {
        let fields = procedure
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
            .collect::<Vec<_>>();
        interfaces.push(build_interface(
            &crate::naming::procedure_wrapper_name(procedure, &occupied_type_names),
            &fields.iter().collect::<Vec<_>>(),
            InterfaceKind::Plain,
            &enum_names,
        ));
    }

    let models = schema
        .models
        .iter()
        .map(build_model_api)
        .collect::<Vec<_>>();
    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect::<Vec<_>>();
    let query_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "query")
        .cloned()
        .collect();
    let mutation_procedures = procedures
        .iter()
        .filter(|procedure| procedure.kind == "mutation")
        .cloned()
        .collect();

    TemplateContext {
        package_name: config.package_name.clone(),
        client_class_name,
        base_path: config.base_path.clone(),
        enums,
        interfaces,
        models,
        procedures,
        query_procedures,
        mutation_procedures,
    }
}
