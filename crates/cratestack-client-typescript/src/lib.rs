use std::collections::BTreeSet;
use std::path::PathBuf;

use cratestack_codegen_template::{
    build_environment, render_package, GeneratedFile, GeneratedPackage, TemplateError,
    TemplateSpec,
};
use cratestack_core::{
    EnumDecl, Field, Model, Procedure, ProcedureKind, Schema, TypeArity, TypeRef,
};
use serde::Serialize;

const TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "package.json.j2",
        output_path: "package.json",
        default_source: include_str!("../templates/package.json.j2"),
    },
    TemplateSpec {
        template_name: "tsconfig.json.j2",
        output_path: "tsconfig.json",
        default_source: include_str!("../templates/tsconfig.json.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: "README.md",
        default_source: include_str!("../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "runtime.ts.j2",
        output_path: "src/runtime.ts",
        default_source: include_str!("../templates/src/runtime.ts.j2"),
    },
    TemplateSpec {
        template_name: "queries.ts.j2",
        output_path: "src/queries.ts",
        default_source: include_str!("../templates/src/queries.ts.j2"),
    },
    TemplateSpec {
        template_name: "models.ts.j2",
        output_path: "src/models.ts",
        default_source: include_str!("../templates/src/models.ts.j2"),
    },
    TemplateSpec {
        template_name: "client.ts.j2",
        output_path: "src/client.ts",
        default_source: include_str!("../templates/src/client.ts.j2"),
    },
    TemplateSpec {
        template_name: "react-query.ts.j2",
        output_path: "src/react-query.ts",
        default_source: include_str!("../templates/src/react-query.ts.j2"),
    },
    TemplateSpec {
        template_name: "index.ts.j2",
        output_path: "src/index.ts",
        default_source: include_str!("../templates/src/index.ts.j2"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptGeneratorConfig {
    pub package_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
}

impl Default for TypeScriptGeneratorConfig {
    fn default() -> Self {
        Self {
            package_name: "cratestack-client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
        }
    }
}

/// Alias for the shared `GeneratedFile` shape — preserved so downstream
/// consumers don't have to change their imports.
pub type GeneratedTypeScriptFile = GeneratedFile;

/// Alias for the shared `GeneratedPackage` shape.
pub type GeneratedTypeScriptPackage = GeneratedPackage;

/// Alias for the shared `TemplateError`; kept for callers that match on it.
pub type TypeScriptGeneratorError = TemplateError;

pub fn generate_package(
    schema: &Schema,
    config: &TypeScriptGeneratorConfig,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    let environment = build_environment(TEMPLATE_SPECS, config.template_dir.as_deref(), |_| {})?;
    let context = build_template_context(schema, config);
    render_package(TEMPLATE_SPECS, &environment, &context, |path| {
        path.to_owned()
    })
}

fn build_template_context(schema: &Schema, config: &TypeScriptGeneratorConfig) -> TemplateContext {
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
            &procedure_wrapper_name(procedure, &occupied_type_names),
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

#[derive(Debug, Clone, Serialize)]
struct TemplateContext {
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

#[derive(Debug, Clone, Serialize)]
struct EnumView {
    name: String,
    union: String,
    values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct InterfaceView {
    name: String,
    has_fields: bool,
    fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
struct FieldView {
    property: String,
    wire_name: String,
    type_name: String,
    optional: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ModelApiView {
    name: String,
    api_name: String,
    accessor: String,
    route: String,
    primary_key_type: String,
    create_input_name: String,
    update_input_name: String,
    list_return_type: String,
    list_query_key: String,
    get_query_key: String,
    create_mutation_key: String,
    update_mutation_key: String,
    delete_mutation_key: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProcedureView {
    name: String,
    method_name: String,
    hook_name: String,
    args_name: String,
    return_type: String,
    route: String,
    kind: &'static str,
    query_key: String,
    mutation_key: String,
}

#[derive(Clone, Copy)]
enum InterfaceKind {
    Plain,
    Patch,
    Model,
}

fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
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

fn build_interface(
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

fn build_model_api(model: &Model) -> ModelApiView {
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

fn build_procedure(
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

fn ts_type(type_ref: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        return format!("Page<{}>", ts_type(item, enum_names));
    }

    let base = match type_ref.name.as_str() {
        "String" | "Cuid" | "Uuid" | "DateTime" => "string".to_owned(),
        "Int" | "Float" => "number".to_owned(),
        "Boolean" => "boolean".to_owned(),
        "Json" => "JsonValue".to_owned(),
        "Bytes" => "number[]".to_owned(),
        other if enum_names.contains(other) => other.to_owned(),
        other => other.to_owned(),
    };

    match type_ref.arity {
        TypeArity::Required => base,
        TypeArity::Optional => format!("{base} | null"),
        TypeArity::List => format!("{base}[]"),
    }
}

fn occupied_type_names(schema: &Schema) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for ty in &schema.types {
        names.insert(ty.name.clone());
    }
    for enum_decl in &schema.enums {
        names.insert(enum_decl.name.clone());
    }
    for model in &schema.models {
        names.insert(model.name.clone());
        names.insert(format!("Create{}Input", model.name));
        names.insert(format!("Update{}Input", model.name));
    }
    names
}

fn procedure_wrapper_name(procedure: &Procedure, occupied_type_names: &BTreeSet<String>) -> String {
    let base = format!("{}Args", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&base) {
        return base;
    }

    let procedure_name = format!("{}ProcedureArgs", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&procedure_name) {
        return procedure_name;
    }

    format!("{}ProcedureRequest", to_pascal_case(&procedure.name))
}

fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

fn scalar_model_fields<'a>(model: &'a Model, model_names: &BTreeSet<&str>) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field))
        .collect()
}

fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

fn primary_key_field(model: &Model) -> Option<&Field> {
    model.fields.iter().find(|field| is_primary_key(field))
}

fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}

fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

fn ts_identifier(value: &str) -> String {
    if is_ts_keyword(value) {
        format!("{value}_")
    } else {
        value.to_owned()
    }
}

fn is_ts_keyword(value: &str) -> bool {
    matches!(
        value,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

fn to_camel_case(value: &str) -> String {
    let pascal = to_pascal_case(value);
    let mut chars = pascal.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_lowercase().collect::<String>() + chars.as_str()
}

fn package_class_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect()
}

fn to_pascal_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        })
        .collect::<String>()
}

fn to_snake_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            continue;
        }

        if ch.is_ascii_uppercase() && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }

        current.push(ch);
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else if value.ends_with('y')
        && !matches!(
            value.chars().rev().nth(1),
            Some('a' | 'e' | 'i' | 'o' | 'u')
        )
    {
        format!("{}ies", &value[..value.len() - 1])
    } else {
        format!("{value}s")
    }
}

fn escape_ts_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
