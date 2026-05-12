use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cratestack_codegen_template::{
    build_environment, render_package, GeneratedFile, GeneratedPackage, TemplateError,
    TemplateSpec,
};
use cratestack_core::{Model, Procedure, ProcedureKind, Schema, TypeArity, TypeRef};
use serde::Serialize;

const TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "root/Cargo.toml.j2",
        output_path: "Cargo.toml",
        default_source: include_str!("../templates/root/Cargo.toml.j2"),
    },
    TemplateSpec {
        template_name: "root/README.md.j2",
        output_path: "README.md",
        default_source: include_str!("../templates/root/README.md.j2"),
    },
    TemplateSpec {
        template_name: "root/Dockerfile.j2",
        output_path: "Dockerfile",
        default_source: include_str!("../templates/root/Dockerfile.j2"),
    },
    TemplateSpec {
        template_name: "root/.gitignore.j2",
        output_path: ".gitignore",
        default_source: include_str!("../templates/root/.gitignore.j2"),
    },
    TemplateSpec {
        template_name: "shared/Cargo.toml.j2",
        output_path: "shared/Cargo.toml",
        default_source: include_str!("../templates/shared/Cargo.toml.j2"),
    },
    TemplateSpec {
        template_name: "shared/src/lib.rs.j2",
        output_path: "shared/src/lib.rs",
        default_source: include_str!("../templates/shared/src/lib.rs.j2"),
    },
    TemplateSpec {
        template_name: "shared/src/metadata.json.j2",
        output_path: "shared/src/metadata.json",
        default_source: include_str!("../templates/shared/src/metadata.json.j2"),
    },
    TemplateSpec {
        template_name: "backend/Cargo.toml.j2",
        output_path: "backend/Cargo.toml",
        default_source: include_str!("../templates/backend/Cargo.toml.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/main.rs.j2",
        output_path: "backend/src/main.rs",
        default_source: include_str!("../templates/backend/src/main.rs.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/config.rs.j2",
        output_path: "backend/src/config.rs",
        default_source: include_str!("../templates/backend/src/config.rs.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/http.rs.j2",
        output_path: "backend/src/http.rs",
        default_source: include_str!("../templates/backend/src/http.rs.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/metadata.rs.j2",
        output_path: "backend/src/metadata.rs",
        default_source: include_str!("../templates/backend/src/metadata.rs.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/proxy.rs.j2",
        output_path: "backend/src/proxy.rs",
        default_source: include_str!("../templates/backend/src/proxy.rs.j2"),
    },
    TemplateSpec {
        template_name: "backend/src/static_files.rs.j2",
        output_path: "backend/src/static_files.rs",
        default_source: include_str!("../templates/backend/src/static_files.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/Cargo.toml.j2",
        output_path: "web/Cargo.toml",
        default_source: include_str!("../templates/web/Cargo.toml.j2"),
    },
    TemplateSpec {
        template_name: "web/package.json.j2",
        output_path: "web/package.json",
        default_source: include_str!("../templates/web/package.json.j2"),
    },
    TemplateSpec {
        template_name: "web/Trunk.toml.j2",
        output_path: "web/Trunk.toml",
        default_source: include_str!("../templates/web/Trunk.toml.j2"),
    },
    TemplateSpec {
        template_name: "web/index.html.j2",
        output_path: "web/index.html",
        default_source: include_str!("../templates/web/index.html.j2"),
    },
    TemplateSpec {
        template_name: "web/src/main.rs.j2",
        output_path: "web/src/main.rs",
        default_source: include_str!("../templates/web/src/main.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/tailwind.css.j2",
        output_path: "web/src/tailwind.css",
        default_source: include_str!("../templates/web/src/tailwind.css.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/mod.rs.j2",
        output_path: "web/src/components/mod.rs",
        default_source: include_str!("../templates/web/src/components/mod.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/icons.rs.j2",
        output_path: "web/src/components/icons.rs",
        default_source: include_str!("../templates/web/src/components/icons.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/mod.rs.j2",
        output_path: "web/src/pages/mod.rs",
        default_source: include_str!("../templates/web/src/pages/mod.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/app.rs.j2",
        output_path: "web/src/app.rs",
        default_source: include_str!("../templates/web/src/app.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/api.rs.j2",
        output_path: "web/src/api.rs",
        default_source: include_str!("../templates/web/src/api.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/state.rs.j2",
        output_path: "web/src/state.rs",
        default_source: include_str!("../templates/web/src/state.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/routes.rs.j2",
        output_path: "web/src/routes.rs",
        default_source: include_str!("../templates/web/src/routes.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/layout.rs.j2",
        output_path: "web/src/components/layout.rs",
        default_source: include_str!("../templates/web/src/components/layout.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/sidebar.rs.j2",
        output_path: "web/src/components/sidebar.rs",
        default_source: include_str!("../templates/web/src/components/sidebar.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/metadata_drawer.rs.j2",
        output_path: "web/src/components/metadata_drawer.rs",
        default_source: include_str!("../templates/web/src/components/metadata_drawer.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/json_view.rs.j2",
        output_path: "web/src/components/json_view.rs",
        default_source: include_str!("../templates/web/src/components/json_view.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/components/table.rs.j2",
        output_path: "web/src/components/table.rs",
        default_source: include_str!("../templates/web/src/components/table.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/schema.rs.j2",
        output_path: "web/src/pages/schema.rs",
        default_source: include_str!("../templates/web/src/pages/schema.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/model_list.rs.j2",
        output_path: "web/src/pages/model_list.rs",
        default_source: include_str!("../templates/web/src/pages/model_list.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/procedures.rs.j2",
        output_path: "web/src/pages/procedures.rs",
        default_source: include_str!("../templates/web/src/pages/procedures.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/query.rs.j2",
        output_path: "web/src/pages/query.rs",
        default_source: include_str!("../templates/web/src/pages/query.rs.j2"),
    },
    TemplateSpec {
        template_name: "web/src/pages/api.rs.j2",
        output_path: "web/src/pages/api.rs",
        default_source: include_str!("../templates/web/src/pages/api.rs.j2"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioGeneratorConfig {
    pub name: String,
    pub mount_path: String,
    pub profile: StudioProfile,
    pub template_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioGeneratorContext<'a> {
    pub key: String,
    pub display_name: String,
    pub service_name: String,
    pub schema_path: PathBuf,
    pub service_url: String,
    pub schema: &'a Schema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioProfile {
    Dev,
    Prod,
}

impl StudioProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Prod => "prod",
        }
    }

    fn enable_dev_context_override_default(self) -> bool {
        matches!(self, Self::Dev)
    }
}

/// Alias for the shared `GeneratedFile` shape — preserved so downstream
/// consumers don't have to change their imports.
pub type GeneratedStudioFile = GeneratedFile;

/// Alias for the shared `GeneratedPackage` shape.
pub type GeneratedStudioPackage = GeneratedPackage;

/// Alias for the shared `TemplateError`; kept for callers that match on it.
pub type StudioGeneratorError = TemplateError;

pub fn generate_package(
    contexts: &[StudioGeneratorContext<'_>],
    config: &StudioGeneratorConfig,
) -> Result<GeneratedStudioPackage, StudioGeneratorError> {
    let environment = build_environment(TEMPLATE_SPECS, config.template_dir.as_deref(), |env| {
        // Studio templates render embedded JSON literals into `.rs.j2`
        // files (metadata dumps, default state). `tojson` is the only
        // generator-specific filter the framework adds.
        env.add_filter("tojson", |value: minijinja::value::Value| {
            serde_json::to_string(&value).map_err(|error| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("failed to render json literal: {error}"),
                )
            })
        });
    })?;
    let context = build_template_context(contexts, config);
    render_package(TEMPLATE_SPECS, &environment, &context, |path| {
        path.to_owned()
    })
}

fn build_template_context(
    contexts: &[StudioGeneratorContext<'_>],
    config: &StudioGeneratorConfig,
) -> TemplateContext {
    assert!(
        !contexts.is_empty(),
        "studio generator requires at least one schema context"
    );

    let workspace_name = slugify(&config.name);
    let backend_package = format!("{workspace_name}-backend");
    let web_package = format!("{workspace_name}-web");
    let shared_package = format!("{workspace_name}-shared");
    let mount_path = normalize_mount_path(&config.mount_path);
    let api_mount_path = format!("{mount_path}/api");
    let assets_mount_path = format!("{mount_path}/assets");
    let contexts = contexts
        .iter()
        .map(build_studio_context)
        .collect::<Vec<_>>();
    let default_context_key = contexts
        .first()
        .map(|context| context.key.clone())
        .unwrap_or_default();
    let flags = FlagsContext {
        has_models: contexts.iter().any(|context| !context.models.is_empty()),
        has_enums: contexts.iter().any(|context| !context.enums.is_empty()),
        has_procedures: contexts
            .iter()
            .any(|context| !context.procedures.is_empty()),
    };

    TemplateContext {
        app: AppContext {
            name: workspace_name.clone(),
            mount_path: mount_path.clone(),
            api_mount_path: api_mount_path.clone(),
            assets_mount_path,
            profile: config.profile.as_str().to_owned(),
            default_context_key: default_context_key.clone(),
        },
        workspace: WorkspaceContext {
            workspace_name,
            backend_crate_name: backend_package.clone(),
            backend_binary_name: backend_package.clone(),
            web_crate_name: web_package,
            shared_crate_name: shared_package.clone(),
            shared_crate_lib_name: shared_package.replace('-', "_"),
            rust_edition: "2024".to_owned(),
        },
        backend: BackendContext {
            bind_addr_default: "127.0.0.1:3000".to_owned(),
            enable_dev_context_override_default: config
                .profile
                .enable_dev_context_override_default(),
            healthz_path: "/healthz".to_owned(),
            metadata_path: format!("{api_mount_path}/metadata"),
            static_dir_default: "web/dist".to_owned(),
        },
        web: WebContext {
            app_title: config.name.clone(),
            metadata_path: format!("{api_mount_path}/metadata"),
        },
        contexts: contexts.clone(),
        metadata_json: serde_json::to_string_pretty(&serde_json::json!({
            "name": config.name,
            "mount_path": mount_path,
            "default_context": default_context_key,
            "contexts": contexts,
        }))
        .expect("metadata json should serialize"),
        flags,
    }
}

fn build_studio_context(context: &StudioGeneratorContext<'_>) -> StudioContextTemplate {
    let model_names = context
        .schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_names = context
        .schema
        .types
        .iter()
        .map(|ty| ty.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_lookup = context
        .schema
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();

    StudioContextTemplate {
        key: context.key.clone(),
        display_name: context.display_name.clone(),
        service: context.service_name.clone(),
        schema_path: context.schema_path.display().to_string(),
        service_url: context.service_url.clone(),
        models: context
            .schema
            .models
            .iter()
            .map(|model| build_model_context(model, &model_names, &context.schema.enums))
            .collect(),
        enums: context
            .schema
            .enums
            .iter()
            .map(|enum_decl| StudioEnumContext {
                name: enum_decl.name.clone(),
                values: enum_decl
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect(),
            })
            .collect(),
        procedures: context
            .schema
            .procedures
            .iter()
            .map(|procedure| {
                build_procedure_context(
                    procedure,
                    &model_names,
                    &type_names,
                    &type_lookup,
                    &context.schema.enums,
                )
            })
            .collect(),
    }
}

fn build_model_context(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enums: &[cratestack_core::EnumDecl],
) -> StudioModelContext {
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .map(|field| field.name.clone())
        .unwrap_or_else(|| "id".to_owned());
    let relations = model
        .fields
        .iter()
        .filter(|field| model_names.contains(field.ty.name.as_str()))
        .map(|field| StudioRelationContext {
            name: field.name.clone(),
            target_model: field.ty.name.clone(),
            cardinality: if field.ty.arity == TypeArity::List {
                "many".to_owned()
            } else {
                "one".to_owned()
            },
        })
        .collect::<Vec<_>>();
    let scalar_fields = model
        .fields
        .iter()
        .filter(|field| !model_names.contains(field.ty.name.as_str()))
        .map(|field| build_field_context(field, enums))
        .collect::<Vec<_>>();

    StudioModelContext {
        name: model.name.clone(),
        display_name: model.name.clone(),
        resource_path: format!("/{}", pluralize(&to_snake_case(&model.name))),
        primary_key: primary_key.clone(),
        paged: is_paged_model(model),
        scalar_fields,
        relations,
        list_columns: model
            .fields
            .iter()
            .filter(|field| !model_names.contains(field.ty.name.as_str()))
            .take(6)
            .map(|field| field.name.clone())
            .collect(),
    }
}

fn build_field_context(
    field: &cratestack_core::Field,
    enums: &[cratestack_core::EnumDecl],
) -> StudioFieldContext {
    let enum_decl = enums
        .iter()
        .find(|enum_decl| enum_decl.name == field.ty.name);
    StudioFieldContext {
        name: field.name.clone(),
        display_name: field.name.clone(),
        type_name: field.ty.name.clone(),
        required: field.ty.arity == TypeArity::Required,
        list: field.ty.arity == TypeArity::List,
        is_json: field.ty.name == "Json",
        enum_name: enum_decl.map(|enum_decl| enum_decl.name.clone()),
        enum_values: enum_decl
            .map(|enum_decl| {
                enum_decl
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn build_procedure_context(
    procedure: &Procedure,
    model_names: &BTreeSet<&str>,
    type_names: &BTreeSet<&str>,
    type_lookup: &BTreeMap<&str, &cratestack_core::TypeDecl>,
    enums: &[cratestack_core::EnumDecl],
) -> StudioProcedureContext {
    let (payload_mode, input_fields) = build_procedure_input_fields(procedure, type_lookup, enums);

    StudioProcedureContext {
        name: procedure.name.clone(),
        display_name: procedure.name.clone(),
        route_path: format!("/$procs/{}", procedure.name),
        kind: match procedure.kind {
            ProcedureKind::Query => "query".to_owned(),
            ProcedureKind::Mutation => "mutation".to_owned(),
        },
        args_type: procedure
            .args
            .iter()
            .find(|arg| arg.name == "args")
            .map(|arg| arg.ty.name.clone()),
        payload_mode,
        input_fields,
        return_type: procedure.return_type.name.clone(),
        return_kind: procedure_return_kind(&procedure.return_type, model_names, type_names),
    }
}

fn build_procedure_input_fields(
    procedure: &Procedure,
    type_lookup: &BTreeMap<&str, &cratestack_core::TypeDecl>,
    enums: &[cratestack_core::EnumDecl],
) -> (String, Vec<StudioInputFieldContext>) {
    if procedure.args.is_empty() {
        return ("empty".to_owned(), Vec::new());
    }

    if procedure.args.len() == 1 && procedure.args[0].name == "args" {
        let arg = &procedure.args[0];
        if let Some(type_decl) = type_lookup.get(arg.ty.name.as_str()) {
            return (
                "wrapped_args_object".to_owned(),
                type_decl
                    .fields
                    .iter()
                    .map(|field| build_procedure_field_context(field, enums))
                    .collect(),
            );
        }
    }

    (
        "flat".to_owned(),
        procedure
            .args
            .iter()
            .map(|arg| build_procedure_arg_context(arg, enums))
            .collect(),
    )
}

fn build_procedure_arg_context(
    arg: &cratestack_core::ProcedureArg,
    enums: &[cratestack_core::EnumDecl],
) -> StudioInputFieldContext {
    let enum_decl = enums.iter().find(|enum_decl| enum_decl.name == arg.ty.name);
    StudioInputFieldContext {
        name: arg.name.clone(),
        display_name: arg.name.clone(),
        type_name: arg.ty.name.clone(),
        required: arg.ty.arity == TypeArity::Required,
        list: arg.ty.arity == TypeArity::List,
        is_json: arg.ty.name == "Json",
        is_boolean: arg.ty.name == "Boolean",
        is_number: matches!(arg.ty.name.as_str(), "Int" | "Float"),
        enum_name: enum_decl.as_ref().map(|enum_decl| enum_decl.name.clone()),
        enum_values: enum_decl
            .map(|enum_decl| {
                enum_decl
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn build_procedure_field_context(
    field: &cratestack_core::Field,
    enums: &[cratestack_core::EnumDecl],
) -> StudioInputFieldContext {
    let enum_decl = enums
        .iter()
        .find(|enum_decl| enum_decl.name == field.ty.name);
    StudioInputFieldContext {
        name: field.name.clone(),
        display_name: field.name.clone(),
        type_name: field.ty.name.clone(),
        required: field.ty.arity == TypeArity::Required,
        list: field.ty.arity == TypeArity::List,
        is_json: field.ty.name == "Json",
        is_boolean: field.ty.name == "Boolean",
        is_number: matches!(field.ty.name.as_str(), "Int" | "Float"),
        enum_name: enum_decl.as_ref().map(|enum_decl| enum_decl.name.clone()),
        enum_values: enum_decl
            .map(|enum_decl| {
                enum_decl
                    .variants
                    .iter()
                    .map(|variant| variant.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn procedure_return_kind(
    ty: &TypeRef,
    model_names: &BTreeSet<&str>,
    type_names: &BTreeSet<&str>,
) -> String {
    if ty.is_page() {
        return "page".to_owned();
    }
    if ty.arity == TypeArity::List {
        return "list".to_owned();
    }
    if model_names.contains(ty.name.as_str()) {
        return "model".to_owned();
    }
    if type_names.contains(ty.name.as_str()) {
        return "type".to_owned();
    }
    "scalar".to_owned()
}

fn normalize_mount_path(mount_path: &str) -> String {
    let trimmed = mount_path.trim();
    trimmed.trim_end_matches('/').to_owned()
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowercase in character.to_lowercase() {
                output.push(lowercase);
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

fn is_primary_key(field: &cratestack_core::Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

#[derive(Debug, Clone, Serialize)]
struct TemplateContext {
    app: AppContext,
    workspace: WorkspaceContext,
    backend: BackendContext,
    web: WebContext,
    contexts: Vec<StudioContextTemplate>,
    metadata_json: String,
    flags: FlagsContext,
}

#[derive(Debug, Clone, Serialize)]
struct AppContext {
    name: String,
    mount_path: String,
    api_mount_path: String,
    assets_mount_path: String,
    profile: String,
    default_context_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceContext {
    workspace_name: String,
    backend_crate_name: String,
    backend_binary_name: String,
    web_crate_name: String,
    shared_crate_name: String,
    shared_crate_lib_name: String,
    rust_edition: String,
}

#[derive(Debug, Clone, Serialize)]
struct BackendContext {
    bind_addr_default: String,
    enable_dev_context_override_default: bool,
    healthz_path: String,
    metadata_path: String,
    static_dir_default: String,
}

#[derive(Debug, Clone, Serialize)]
struct WebContext {
    app_title: String,
    metadata_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct StudioContextTemplate {
    key: String,
    display_name: String,
    service: String,
    schema_path: String,
    service_url: String,
    models: Vec<StudioModelContext>,
    enums: Vec<StudioEnumContext>,
    procedures: Vec<StudioProcedureContext>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioModelContext {
    name: String,
    display_name: String,
    resource_path: String,
    primary_key: String,
    paged: bool,
    scalar_fields: Vec<StudioFieldContext>,
    relations: Vec<StudioRelationContext>,
    list_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioFieldContext {
    name: String,
    display_name: String,
    type_name: String,
    required: bool,
    list: bool,
    is_json: bool,
    enum_name: Option<String>,
    enum_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioRelationContext {
    name: String,
    target_model: String,
    cardinality: String,
}

#[derive(Debug, Clone, Serialize)]
struct StudioEnumContext {
    name: String,
    values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StudioProcedureContext {
    name: String,
    display_name: String,
    route_path: String,
    kind: String,
    args_type: Option<String>,
    payload_mode: String,
    input_fields: Vec<StudioInputFieldContext>,
    return_type: String,
    return_kind: String,
}

#[derive(Debug, Clone, Serialize)]
struct StudioInputFieldContext {
    name: String,
    display_name: String,
    type_name: String,
    required: bool,
    list: bool,
    is_json: bool,
    is_boolean: bool,
    is_number: bool,
    enum_name: Option<String>,
    enum_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct FlagsContext {
    has_models: bool,
    has_enums: bool,
    has_procedures: bool,
}
