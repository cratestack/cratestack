use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use minijinja::Environment;
use serde::Serialize;

use cratestack_core::{EnumDecl, Field, Model, Procedure, Schema, TypeArity, TypeRef};

fn synthetic_span() -> cratestack_core::SourceSpan {
    cratestack_core::SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

const TEMPLATE_SPECS: &[TemplateSpec] = &[
    TemplateSpec {
        template_name: "pubspec.yaml.j2",
        output_path: "pubspec.yaml",
        default_source: include_str!("../templates/pubspec.yaml.j2"),
    },
    TemplateSpec {
        template_name: "README.md.j2",
        output_path: "README.md",
        default_source: include_str!("../templates/README.md.j2"),
    },
    TemplateSpec {
        template_name: "CHANGELOG.md.j2",
        output_path: "CHANGELOG.md",
        default_source: include_str!("../templates/CHANGELOG.md.j2"),
    },
    TemplateSpec {
        template_name: "analysis_options.yaml.j2",
        output_path: "analysis_options.yaml",
        default_source: include_str!("../templates/analysis_options.yaml.j2"),
    },
    TemplateSpec {
        template_name: "library.dart.j2",
        output_path: "lib/{{ package_name }}.dart",
        default_source: include_str!("../templates/library.dart.j2"),
    },
    TemplateSpec {
        template_name: "runtime.dart.j2",
        output_path: "lib/src/runtime.dart",
        default_source: include_str!("../templates/runtime.dart.j2"),
    },
    TemplateSpec {
        template_name: "queries.dart.j2",
        output_path: "lib/src/queries.dart",
        default_source: include_str!("../templates/queries.dart.j2"),
    },
    TemplateSpec {
        template_name: "constants.dart.j2",
        output_path: "lib/src/constants.dart",
        default_source: include_str!("../templates/constants.dart.j2"),
    },
    TemplateSpec {
        template_name: "models.dart.j2",
        output_path: "lib/src/models.dart",
        default_source: include_str!("../templates/models.dart.j2"),
    },
    TemplateSpec {
        template_name: "apis.dart.j2",
        output_path: "lib/src/apis.dart",
        default_source: include_str!("../templates/apis.dart.j2"),
    },
    TemplateSpec {
        template_name: "example_main.dart.j2",
        output_path: "example/main.dart",
        default_source: include_str!("../templates/example_main.dart.j2"),
    },
    TemplateSpec {
        template_name: "package_test.dart.j2",
        output_path: "test/{{ package_name }}_test.dart",
        default_source: include_str!("../templates/package_test.dart.j2"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGeneratorConfig {
    pub library_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartFile {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartPackage {
    pub files: Vec<GeneratedDartFile>,
}

#[derive(Debug, Clone, Copy)]
struct TemplateSpec {
    template_name: &'static str,
    output_path: &'static str,
    default_source: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum DartGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
}

pub fn generate_package(
    schema: &Schema,
    config: &DartGeneratorConfig,
) -> Result<GeneratedDartPackage, DartGeneratorError> {
    let environment = build_environment(config.template_dir.as_deref())?;
    let context = build_template_context(schema, config);
    let files = TEMPLATE_SPECS
        .iter()
        .map(|spec| {
            let template = environment
                .get_template(spec.template_name)
                .map_err(|error| DartGeneratorError::TemplateRender(spec.template_name, error))?;
            let contents = template
                .render(&context)
                .map_err(|error| DartGeneratorError::TemplateRender(spec.template_name, error))?;
            Ok(GeneratedDartFile {
                file_name: spec
                    .output_path
                    .replace("{{ package_name }}", &context.package_name),
                contents,
            })
        })
        .collect::<Result<Vec<_>, DartGeneratorError>>()?;

    Ok(GeneratedDartPackage { files })
}

fn build_environment(
    template_dir: Option<&Path>,
) -> Result<Environment<'static>, DartGeneratorError> {
    let mut environment = Environment::new();
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);

    for spec in TEMPLATE_SPECS {
        let source = load_template_source(template_dir, spec)?;
        environment
            .add_template_owned(spec.template_name.to_owned(), source)
            .map_err(|error| DartGeneratorError::TemplateRegistration(spec.template_name, error))?;
    }

    Ok(environment)
}

fn load_template_source(
    template_dir: Option<&Path>,
    spec: &TemplateSpec,
) -> Result<String, DartGeneratorError> {
    let Some(template_dir) = template_dir else {
        return Ok(spec.default_source.to_owned());
    };
    let path = template_dir.join(spec.template_name);
    if !path.exists() {
        return Ok(spec.default_source.to_owned());
    }

    fs::read_to_string(&path).map_err(|source| DartGeneratorError::TemplateRead {
        path: path.display().to_string(),
        template_name: spec.template_name,
        source,
    })
}

fn build_template_context(schema: &Schema, config: &DartGeneratorConfig) -> TemplateContext {
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

#[derive(Debug, Clone, Serialize)]
struct TemplateContext {
    package_name: String,
    client_class_name: String,
    provider_prefix: String,
    base_path_literal: String,
    enum_types: Vec<EnumView>,
    data_classes: Vec<DataClassView>,
    selection_groups: Vec<SelectionGroupView>,
    selection_models: Vec<SelectionModelView>,
    model_accessors: Vec<ModelAccessorView>,
    model_apis: Vec<ModelApiView>,
    procedures: Vec<ProcedureView>,
    query_procedures: Vec<ProcedureView>,
    mutation_procedures: Vec<ProcedureView>,
    sample_model: Option<SampleModelView>,
}

#[derive(Debug, Clone, Serialize)]
struct EnumView {
    name: String,
    variants: Vec<EnumVariantView>,
}

#[derive(Debug, Clone, Serialize)]
struct EnumVariantView {
    identifier: String,
    wire_name: String,
}

#[derive(Debug, Clone, Serialize)]
struct DataClassView {
    name: String,
    has_fields: bool,
    fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
struct FieldView {
    identifier: String,
    wire_name: String,
    dart_type: String,
    required: bool,
    from_wire_expr: String,
    to_wire_expr: String,
}

#[derive(Debug, Clone, Serialize)]
struct SelectionGroupView {
    field_group_name: String,
    fields: Vec<ConstantView>,
    include_group_name: String,
    includes: Vec<ConstantView>,
}

#[derive(Debug, Clone, Serialize)]
struct ConstantView {
    const_name: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
struct ModelAccessorView {
    accessor: String,
    api_class_name: String,
    provider_name: String,
}

#[derive(Debug, Clone, Serialize)]
struct SampleModelView {
    model_name: String,
    accessor: String,
    field_group_name: String,
    include_group_name: String,
    first_field: Option<ConstantView>,
    first_include: Option<ConstantView>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelApiView {
    api_class_name: String,
    model_name: String,
    create_input_name: String,
    update_input_name: String,
    route: String,
    detail_route: String,
    primary_key_type: String,
    is_paged: bool,
    list_return_type: String,
    list_decode_expr: String,
}

#[derive(Debug, Clone, Serialize)]
struct SelectionModelView {
    model_name: String,
    selection_class_name: String,
    include_selection_class_name: String,
    projected_class_name: String,
    scalar_fields: Vec<SelectedFieldAccessorView>,
    relations: Vec<SelectedRelationAccessorView>,
}

#[derive(Debug, Clone, Serialize)]
struct SelectedFieldAccessorView {
    identifier: String,
    wire_name: String,
    dart_type: String,
    decode_expr: String,
}

#[derive(Debug, Clone, Serialize)]
struct SelectedRelationAccessorView {
    identifier: String,
    wire_name: String,
    target_selection_class_name: String,
    target_include_selection_class_name: String,
    target_projected_class_name: String,
    is_list: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ProcedureView {
    method_name: String,
    args_name: String,
    return_type: String,
    route: String,
    return_decode_expr: String,
    kind: &'static str,
}

#[derive(Clone, Copy)]
enum DataClassKind {
    Plain,
    Patch,
    ProjectionModel,
}

fn build_enum_view(enum_decl: &EnumDecl) -> EnumView {
    EnumView {
        name: enum_decl.name.clone(),
        variants: enum_decl
            .variants
            .iter()
            .map(|variant| EnumVariantView {
                identifier: dart_identifier(&to_camel_case(&variant.name)),
                wire_name: variant.name.clone(),
            })
            .collect(),
    }
}

fn build_data_class(
    name: &str,
    fields: &[&Field],
    kind: DataClassKind,
    enum_names: &BTreeSet<&str>,
) -> DataClassView {
    DataClassView {
        name: name.to_owned(),
        has_fields: !fields.is_empty(),
        fields: fields
            .iter()
            .map(|field| FieldView {
                identifier: dart_identifier(&field.name),
                wire_name: field.name.clone(),
                dart_type: dart_field_type(field, kind),
                required: matches!(kind, DataClassKind::Plain)
                    && field.ty.arity == TypeArity::Required,
                from_wire_expr: decode_value_expr(
                    &format!("value['{}']", field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                    name,
                    &field.name,
                ),
                to_wire_expr: encode_value_expr(
                    &dart_identifier(&field.name),
                    &field.ty,
                    enum_names,
                    matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel),
                ),
            })
            .collect(),
    }
}

fn build_selection_group(model: &Model, model_names: &BTreeSet<&str>) -> SelectionGroupView {
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

fn build_selection_model(
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

fn build_model_api(model: &Model) -> ModelApiView {
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

fn build_procedure(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
    enum_names: &BTreeSet<&str>,
) -> ProcedureView {
    ProcedureView {
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

fn dart_field_type(field: &Field, kind: DataClassKind) -> String {
    let is_nullable = matches!(kind, DataClassKind::Patch | DataClassKind::ProjectionModel)
        || field.ty.arity == TypeArity::Optional;
    dart_type(&field.ty, is_nullable)
}

fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

fn dart_type(type_ref: &TypeRef, force_nullable: bool) -> String {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let base = format!("Page<{}>", dart_type(item, false));
        return if force_nullable {
            format!("{base}?")
        } else {
            base
        };
    }

    let base = match type_ref.name.as_str() {
        "String" | "Cuid" | "Uuid" => "String".to_owned(),
        "Int" => "int".to_owned(),
        "Float" => "double".to_owned(),
        "Boolean" => "bool".to_owned(),
        "DateTime" => "DateTime".to_owned(),
        "Json" => "Object?".to_owned(),
        "Bytes" => "Uint8List".to_owned(),
        other => other.to_owned(),
    };

    match type_ref.arity {
        TypeArity::List => format!("List<{base}>{}", if force_nullable { "?" } else { "" }),
        TypeArity::Required => {
            if force_nullable && base != "Object?" {
                format!("{base}?")
            } else {
                base
            }
        }
        TypeArity::Optional => {
            if base.ends_with('?') {
                base
            } else {
                format!("{base}?")
            }
        }
    }
}

fn decode_value_expr(
    expr: &str,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
    force_nullable: bool,
    owner_name: &str,
    field_name: &str,
) -> String {
    match ty.arity {
        TypeArity::List => {
            if force_nullable {
                let item = decode_required_scalar(
                    "item",
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Required,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                );
                format!(
                    "{expr} == null ? null : cratestackAsValueList({expr}).map((item) => {item}).toList(growable: false)"
                )
            } else {
                let item = decode_required_scalar(
                    "item",
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Required,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                );
                let list_expr =
                    format!("cratestackRequireWireValue('{owner_name}', '{field_name}', {expr})");
                format!(
                    "cratestackAsValueList({list_expr}).map((item) => {item}).toList(growable: false)"
                )
            }
        }
        TypeArity::Optional => decode_optional_scalar(expr, ty, enum_names),
        TypeArity::Required => {
            if force_nullable {
                decode_optional_scalar(
                    expr,
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Optional,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                )
            } else {
                let required_expr =
                    format!("cratestackRequireWireValue('{owner_name}', '{field_name}', {expr})");
                decode_required_scalar(&required_expr, ty, enum_names)
            }
        }
    }
}

fn decode_required_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        let item = ty
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_decode = decode_required_scalar("item", item, enum_names);
        return format!(
            "Page<{}>.fromWire(cratestackAsValueMap({expr}), decodeItem: (item) => {item_decode})",
            dart_type(item, false),
        );
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{}.fromWire({expr})", ty.name);
    }

    match ty.name.as_str() {
        "String" | "Cuid" | "Uuid" => format!("{expr} as String"),
        "Int" => format!("({expr} as num).toInt()"),
        "Float" => format!("({expr} as num).toDouble()"),
        "Boolean" => format!("{expr} as bool"),
        "DateTime" => format!("DateTime.parse({expr} as String)"),
        "Json" => expr.to_owned(),
        "Bytes" => format!("Uint8List.fromList(List<int>.from(cratestackAsValueList({expr})))"),
        other => format!("{other}.fromWire(cratestackAsValueMap({expr}))"),
    }
}

fn decode_optional_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.name == "Json" {
        return expr.to_owned();
    }

    let required = decode_required_scalar(
        expr,
        &TypeRef {
            name: ty.name.clone(),
            name_span: synthetic_span(),
            arity: TypeArity::Required,
            generic_args: ty.generic_args.clone(),
        },
        enum_names,
    );
    format!("{expr} == null ? null : {required}")
}

fn encode_value_expr(
    expr: &str,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
    force_nullable: bool,
) -> String {
    match ty.arity {
        TypeArity::List => {
            let item = encode_required_scalar(
                "item",
                &TypeRef {
                    name: ty.name.clone(),
                    name_span: synthetic_span(),
                    arity: TypeArity::Required,
                    generic_args: ty.generic_args.clone(),
                },
                enum_names,
            );
            if force_nullable {
                format!("{expr}?.map((item) => {item}).toList(growable: false)")
            } else {
                format!("{expr}.map((item) => {item}).toList(growable: false)")
            }
        }
        TypeArity::Optional => encode_optional_scalar(expr, ty, enum_names),
        TypeArity::Required => {
            if force_nullable {
                encode_optional_scalar(
                    expr,
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Optional,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                )
            } else {
                encode_required_scalar(expr, ty, enum_names)
            }
        }
    }
}

fn encode_required_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        return format!("{expr}.toWire()");
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{expr}.toWire()");
    }

    match ty.name.as_str() {
        "DateTime" => format!("{expr}.toUtc().toIso8601String()"),
        "Bytes" => format!("{expr}.toList(growable: false)"),
        "Json" | "String" | "Cuid" | "Uuid" | "Int" | "Float" | "Boolean" => expr.to_owned(),
        _ => format!("{expr}.toWire()"),
    }
}

fn encode_optional_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        return format!("{expr}?.toWire()");
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{expr}?.toWire()");
    }

    match ty.name.as_str() {
        "DateTime" => format!("{expr}?.toUtc().toIso8601String()"),
        "Bytes" => format!("{expr}?.toList(growable: false)"),
        "Json" | "String" | "Cuid" | "Uuid" | "Int" | "Float" | "Boolean" => expr.to_owned(),
        _ => format!("{expr}?.toWire()"),
    }
}

fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
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

fn dart_identifier(value: &str) -> String {
    if is_dart_keyword(value) {
        format!("{value}$")
    } else {
        value.to_owned()
    }
}

fn is_dart_keyword(value: &str) -> bool {
    matches!(
        value,
        "assert"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "extends"
            | "false"
            | "final"
            | "finally"
            | "for"
            | "if"
            | "in"
            | "is"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
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

fn escape_dart_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
