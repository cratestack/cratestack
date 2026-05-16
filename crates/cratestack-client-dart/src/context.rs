use cratestack_core::{Field, Schema};

use crate::builders::{build_data_class, build_enum_view};
use crate::builders_model::{
    build_model_api, build_procedure, build_selection_group, build_selection_model,
};
use crate::config::DartGeneratorConfig;
use crate::idents::{dart_identifier, escape_dart_string, pluralize, to_camel_case, to_pascal_case};
use crate::naming::{
    enum_name_set, is_generated_on_create, is_primary_key, is_relation_field, model_name_set,
    occupied_type_names, procedure_wrapper_name, scalar_model_fields,
};
use crate::views::{
    ConstantView, DataClassKind, ModelAccessorView, SampleModelView, TemplateContext,
};

pub(crate) fn build_template_context(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> TemplateContext {
    let model_names = model_name_set(&schema.models);
    let enum_names = enum_name_set(&schema.enums);
    let occupied_type_names = occupied_type_names(schema);
    let client_class_name = format!("{}CratestackClient", to_pascal_case(&config.library_name));
    let provider_prefix = to_camel_case(&config.library_name);
    let enum_types = schema.enums.iter().map(build_enum_view).collect();

    let mut data_classes = Vec::new();
    for ty in &schema.types {
        let fields = ty.fields.iter().collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &ty.name,
            &fields,
            DataClassKind::Plain,
            &enum_names,
        ));
    }

    for model in &schema.models {
        let model_fields = model.fields.iter().collect::<Vec<_>>();
        let scalar_fields = scalar_model_fields(model, &model_names);
        data_classes.push(build_data_class(
            &model.name,
            &model_fields,
            DataClassKind::ProjectionModel,
            &enum_names,
        ));

        let create_name = format!("Create{}Input", model.name);
        let create_fields = scalar_fields
            .iter()
            .copied()
            .filter(|field| !is_generated_on_create(field))
            .collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &create_name,
            &create_fields,
            DataClassKind::Plain,
            &enum_names,
        ));

        let update_name = format!("Update{}Input", model.name);
        let update_fields = scalar_fields
            .iter()
            .copied()
            .filter(|field| !is_primary_key(field))
            .collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &update_name,
            &update_fields,
            DataClassKind::Patch,
            &enum_names,
        ));
    }

    for procedure in &schema.procedures {
        let args_name = procedure_wrapper_name(procedure, &occupied_type_names);
        let fields = procedure
            .args
            .iter()
            .map(|arg| Field {
                docs: arg.docs.clone(),
                name: arg.name.clone(),
                name_span: arg.name_span,
                ty: arg.ty.clone(),
                attributes: Vec::new(),
                span: procedure.span,
            })
            .collect::<Vec<_>>();
        let field_refs = fields.iter().collect::<Vec<_>>();
        data_classes.push(build_data_class(
            &args_name,
            &field_refs,
            DataClassKind::Plain,
            &enum_names,
        ));
    }

    let selection_groups = schema
        .models
        .iter()
        .map(|model| build_selection_group(model, &model_names))
        .collect();
    let selection_models = schema
        .models
        .iter()
        .map(|model| build_selection_model(model, &schema.models, &model_names, &enum_names))
        .collect();

    let model_accessors = schema
        .models
        .iter()
        .map(|model| ModelAccessorView {
            accessor: pluralize(&to_camel_case(&model.name)),
            api_class_name: format!("{}Api", model.name),
            provider_name: format!("{provider_prefix}{}ApiProvider", model.name),
        })
        .collect();

    let model_apis = schema.models.iter().map(build_model_api).collect();
    let procedures = schema
        .procedures
        .iter()
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let query_procedures = schema
        .procedures
        .iter()
        .filter(|procedure| procedure.kind == cratestack_core::ProcedureKind::Query)
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let mutation_procedures = schema
        .procedures
        .iter()
        .filter(|procedure| procedure.kind == cratestack_core::ProcedureKind::Mutation)
        .map(|procedure| build_procedure(procedure, &occupied_type_names, &enum_names))
        .collect();
    let sample_model = schema.models.first().map(|model| {
        let accessor = pluralize(&to_camel_case(&model.name));
        let field_group_name = format!("{}FieldNames", model.name);
        let include_group_name = format!("{}IncludeNames", model.name);
        let scalar_fields = scalar_model_fields(model, &model_names);
        let first_field = scalar_fields.first().map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        });
        let relation_fields = model
            .fields
            .iter()
            .filter(|field| is_relation_field(&model_names, field))
            .collect::<Vec<_>>();
        let first_include = relation_fields.first().map(|field| ConstantView {
            const_name: dart_identifier(&to_camel_case(&field.name)),
            value: field.name.clone(),
        });

        SampleModelView {
            model_name: model.name.clone(),
            accessor,
            field_group_name,
            include_group_name,
            first_field,
            first_include,
        }
    });

    TemplateContext {
        package_name: config.library_name.clone(),
        client_class_name,
        provider_prefix,
        base_path_literal: escape_dart_string(&config.base_path),
        enum_types,
        data_classes,
        selection_groups,
        selection_models,
        model_accessors,
        model_apis,
        procedures,
        query_procedures,
        mutation_procedures,
        sample_model,
    }
}
