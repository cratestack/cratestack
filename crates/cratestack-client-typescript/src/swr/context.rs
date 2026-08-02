//! Builds the `swr` preset's template contexts (issue #304) from a
//! `Schema` + the ownership computation (`crate::swr::ownership`). The
//! lower-level import/ownership-splitting helpers live in
//! `context_imports.rs` — split out to keep this file under this repo's
//! ~200-LoC convention.

use cratestack_core::{Model, Schema};

use crate::config::TypeScriptGeneratorConfig;
use crate::naming::{model_fn_names, procedure_wrapper_name, to_kebab_case};
use crate::types::{
    enum_name_set, is_generated_on_create, is_primary_key, model_allows_create, model_name_set,
    scalar_model_fields, visible_model_fields,
};
use crate::views::{InterfaceKind, build_interface, build_model_api, build_procedure};

use super::context_imports::{
    build_imports, model_refs_in_fields, owned_by, procedure_arg_fields, procedure_model_refs,
    type_decls_model_refs,
};
use super::ownership::{TypeOwner, TypeOwnership};
use super::views::{SwrModelFileContext, SwrModelSummary, SwrProceduresView, SwrSchemaContext};

pub(crate) fn build_shared_context(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
    ownership: &TypeOwnership,
) -> SwrSchemaContext {
    let enum_names = enum_name_set(&schema.enums);
    let model_names = model_name_set(&schema.models);

    let (shared_enums, shared_interfaces) = owned_by(schema, ownership, &enum_names, |owner| {
        matches!(owner, TypeOwner::Shared)
    });
    let shared_model_refs = type_decls_model_refs(schema, ownership, &model_names, |owner| {
        matches!(owner, TypeOwner::Shared)
    });
    let shared = super::views::SwrSharedView {
        enums: shared_enums,
        interfaces: shared_interfaces,
        imports: build_imports(Vec::new(), shared_model_refs, None, "", "./"),
    };

    let models = schema.models.iter().map(model_summary).collect::<Vec<_>>();

    let (procedures_owned_enums, procedures_owned_interfaces) =
        owned_by(schema, ownership, &enum_names, |owner| {
            matches!(owner, TypeOwner::Procedures)
        });
    let procedures_shared_names = ownership.shared_imports_for_procedures();
    let mut procedures_model_refs = procedure_model_refs(schema, &model_names);
    procedures_model_refs.extend(type_decls_model_refs(
        schema,
        ownership,
        &model_names,
        |owner| matches!(owner, TypeOwner::Procedures),
    ));
    let occupied = crate::naming::occupied_type_names(schema);
    let args_interfaces = schema
        .procedures
        .iter()
        .map(|procedure| {
            let fields = procedure_arg_fields(procedure);
            build_interface(
                &procedure_wrapper_name(procedure, &occupied),
                &fields.iter().collect::<Vec<_>>(),
                InterfaceKind::Plain,
                &enum_names,
            )
        })
        .collect();
    let procedures_file = SwrProceduresView {
        owned_enums: procedures_owned_enums,
        owned_interfaces: procedures_owned_interfaces,
        imports: build_imports(
            procedures_shared_names,
            procedures_model_refs,
            None,
            "./models/shared",
            "./models/",
        ),
        args_interfaces,
        procedures: schema
            .procedures
            .iter()
            .map(|procedure| build_procedure(procedure, &occupied, &enum_names))
            .collect(),
    };

    SwrSchemaContext {
        package_name: config.package_name.clone(),
        base_path: config.base_path.clone(),
        schema_sha256: config.schema_sha256.clone(),
        shared,
        models,
        procedures_file,
    }
}

pub(crate) fn build_model_file_contexts(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
    ownership: &TypeOwnership,
) -> Vec<SwrModelFileContext> {
    let enum_names = enum_name_set(&schema.enums);
    let model_names = model_name_set(&schema.models);
    let model_interface_kind = if config.full_selection {
        InterfaceKind::Plain
    } else {
        InterfaceKind::Model
    };

    schema
        .models
        .iter()
        .map(|model| {
            let scalar_fields = scalar_model_fields(model, &model_names);
            let model_interface = build_interface(
                &model.name,
                &visible_model_fields(model),
                model_interface_kind,
                &enum_names,
            );
            let create_input = model_allows_create(model).then(|| {
                build_interface(
                    &format!("Create{}Input", model.name),
                    &scalar_fields
                        .iter()
                        .copied()
                        .filter(|field| !is_generated_on_create(field))
                        .collect::<Vec<_>>(),
                    InterfaceKind::Plain,
                    &enum_names,
                )
            });
            let update_input = build_interface(
                &format!("Update{}Input", model.name),
                &scalar_fields
                    .iter()
                    .copied()
                    .filter(|field| !is_primary_key(field))
                    .collect::<Vec<_>>(),
                InterfaceKind::Patch,
                &enum_names,
            );

            let (owned_enums, owned_interfaces) = owned_by(
                schema,
                ownership,
                &enum_names,
                |owner| matches!(owner, TypeOwner::Model(name) if name == &model.name),
            );

            let shared_names = ownership.shared_imports_for_model(&model.name);
            let mut model_refs =
                model_refs_in_fields(visible_model_fields(model).into_iter(), &model_names);
            model_refs.extend(type_decls_model_refs(
                schema,
                ownership,
                &model_names,
                |owner| matches!(owner, TypeOwner::Model(name) if name == &model.name),
            ));
            // A relation never needs to import its own model — that type
            // is defined right below in this same file.
            model_refs.remove(&model.name);
            let imports = build_imports(
                shared_names,
                model_refs,
                Some(model.name.as_str()),
                "./shared",
                "./",
            );

            let fns = model_fn_names(&model.name);
            SwrModelFileContext {
                file_stem: to_kebab_case(&model.name),
                model: build_model_api(model),
                model_interface,
                create_input,
                update_input,
                owned_enums,
                owned_interfaces,
                imports,
                list_fn: fns.list,
                get_fn: fns.get,
                create_fn: fns.create,
                update_fn: fns.update,
                delete_fn: fns.delete,
            }
        })
        .collect()
}

fn model_summary(model: &Model) -> SwrModelSummary {
    let api = build_model_api(model);
    let fns = model_fn_names(&model.name);
    SwrModelSummary {
        name: model.name.clone(),
        file_stem: to_kebab_case(&model.name),
        accessor: api.accessor,
        allows_create: api.allows_create,
        list_fn: fns.list,
        get_fn: fns.get,
        create_fn: fns.create,
        update_fn: fns.update,
        delete_fn: fns.delete,
    }
}
