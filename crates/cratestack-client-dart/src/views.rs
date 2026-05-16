use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemplateContext {
    pub(crate) package_name: String,
    pub(crate) client_class_name: String,
    pub(crate) provider_prefix: String,
    pub(crate) base_path_literal: String,
    pub(crate) enum_types: Vec<EnumView>,
    pub(crate) data_classes: Vec<DataClassView>,
    pub(crate) selection_groups: Vec<SelectionGroupView>,
    pub(crate) selection_models: Vec<SelectionModelView>,
    pub(crate) model_accessors: Vec<ModelAccessorView>,
    pub(crate) model_apis: Vec<ModelApiView>,
    pub(crate) procedures: Vec<ProcedureView>,
    pub(crate) query_procedures: Vec<ProcedureView>,
    pub(crate) mutation_procedures: Vec<ProcedureView>,
    pub(crate) sample_model: Option<SampleModelView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumView {
    pub(crate) name: String,
    pub(crate) variants: Vec<EnumVariantView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnumVariantView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DataClassView {
    pub(crate) name: String,
    pub(crate) has_fields: bool,
    pub(crate) fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) dart_type: String,
    pub(crate) required: bool,
    pub(crate) from_wire_expr: String,
    pub(crate) to_wire_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectionGroupView {
    pub(crate) field_group_name: String,
    pub(crate) fields: Vec<ConstantView>,
    pub(crate) include_group_name: String,
    pub(crate) includes: Vec<ConstantView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConstantView {
    pub(crate) const_name: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelAccessorView {
    pub(crate) accessor: String,
    pub(crate) api_class_name: String,
    pub(crate) provider_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SampleModelView {
    pub(crate) model_name: String,
    pub(crate) accessor: String,
    pub(crate) field_group_name: String,
    pub(crate) include_group_name: String,
    pub(crate) first_field: Option<ConstantView>,
    pub(crate) first_include: Option<ConstantView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelApiView {
    pub(crate) api_class_name: String,
    pub(crate) model_name: String,
    pub(crate) create_input_name: String,
    pub(crate) update_input_name: String,
    pub(crate) route: String,
    pub(crate) detail_route: String,
    pub(crate) primary_key_type: String,
    pub(crate) is_paged: bool,
    pub(crate) list_return_type: String,
    pub(crate) list_decode_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectionModelView {
    pub(crate) model_name: String,
    pub(crate) selection_class_name: String,
    pub(crate) include_selection_class_name: String,
    pub(crate) projected_class_name: String,
    pub(crate) scalar_fields: Vec<SelectedFieldAccessorView>,
    pub(crate) relations: Vec<SelectedRelationAccessorView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectedFieldAccessorView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) dart_type: String,
    pub(crate) decode_expr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SelectedRelationAccessorView {
    pub(crate) identifier: String,
    pub(crate) wire_name: String,
    pub(crate) target_selection_class_name: String,
    pub(crate) target_include_selection_class_name: String,
    pub(crate) target_projected_class_name: String,
    pub(crate) is_list: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcedureView {
    /// Raw schema procedure name (e.g. `publishPost`). Used to build
    /// the server-side op id `procedure.<name>` in RPC mode and to
    /// build the REST URL `/$procs/<name>` in REST mode.
    pub(crate) name: String,
    pub(crate) method_name: String,
    pub(crate) args_name: String,
    pub(crate) return_type: String,
    pub(crate) route: String,
    pub(crate) return_decode_expr: String,
    pub(crate) kind: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) enum DataClassKind {
    Plain,
    Patch,
    ProjectionModel,
}
