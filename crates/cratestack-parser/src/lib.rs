mod diagnostics;
mod diagnostics_related;
mod directives;
mod entry;
mod line_helpers;
mod parse;
mod public_lists;
mod relation_actions;
mod relation_helpers;
mod validate;

#[cfg(test)]
mod tests_attribute_run_comments;
#[cfg(test)]
mod tests_attribute_spacing;
#[cfg(test)]
mod tests_attribute_spelling;
#[cfg(test)]
mod tests_attribute_spelling_blocks;
#[cfg(test)]
mod tests_attribute_spelling_comments;
#[cfg(test)]
mod tests_basic;
#[cfg(test)]
mod tests_block_attribute_allowlist;
#[cfg(test)]
mod tests_block_spans;
mod tests_builder_add_setter_collisions;
#[cfg(test)]
mod tests_builder_collisions;
mod tests_builder_collisions_derived;
#[cfg(test)]
mod tests_client_method_collisions;
#[cfg(test)]
mod tests_computed;
#[cfg(test)]
mod tests_computed_params;
mod tests_computed_stream;
mod tests_computed_type_valued;
#[cfg(test)]
mod tests_db_enforce_eligibility;
#[cfg(test)]
mod tests_detached_attributes;
#[cfg(test)]
mod tests_directives;
#[cfg(test)]
mod tests_directives_base;
#[cfg(test)]
mod tests_docs;
#[cfg(test)]
mod tests_enums;
#[cfg(test)]
mod tests_error_file_identity;
#[cfg(test)]
mod tests_extensions;
#[cfg(test)]
mod tests_field_attribute_lists;
#[cfg(test)]
mod tests_field_attribute_lists_accepted;
#[cfg(test)]
mod tests_field_attrs;
#[cfg(test)]
mod tests_format_characters;
#[cfg(test)]
mod tests_format_characters_policy;
#[cfg(test)]
mod tests_hidden_breaks;
#[cfg(test)]
mod tests_isolation_scope;
#[cfg(test)]
mod tests_key_relation_attrs;
#[cfg(test)]
mod tests_key_relation_interplay;
#[cfg(test)]
mod tests_list_arity;
#[cfg(test)]
mod tests_mcp_block;
#[cfg(test)]
mod tests_mcp_inert;
#[cfg(test)]
mod tests_mcp_name;
#[cfg(test)]
mod tests_mcp_name_reserved;
#[cfg(test)]
mod tests_mcp_parse;
#[cfg(test)]
mod tests_mcp_placement;
#[cfg(test)]
mod tests_mcp_rules;
#[cfg(test)]
mod tests_mcp_scope;
#[cfg(test)]
mod tests_mcp_support;
#[cfg(test)]
mod tests_mixins;
#[cfg(test)]
mod tests_model_attrs;
#[cfg(test)]
mod tests_model_index;
#[cfg(test)]
mod tests_model_internal;
#[cfg(test)]
mod tests_model_policy_spelling;
#[cfg(test)]
mod tests_model_unique;
#[cfg(test)]
mod tests_multi_error;
#[cfg(test)]
mod tests_multifile_reserved_keywords;
#[cfg(test)]
mod tests_optional_builtin_args;
mod tests_patch_touch_flag_collisions;
#[cfg(test)]
mod tests_procedure_handler_collisions;
#[cfg(test)]
mod tests_procedure_policy_controls;
#[cfg(test)]
mod tests_procedure_policy_spelling;
#[cfg(test)]
mod tests_procedures;
#[cfg(test)]
mod tests_queries;
#[cfg(test)]
mod tests_queries_attributes;
#[cfg(test)]
mod tests_queries_rejections;
#[cfg(test)]
mod tests_queries_sql_body;
#[cfg(test)]
mod tests_queries_support;
#[cfg(test)]
mod tests_query_policy_spelling;
#[cfg(test)]
mod tests_related_render;
#[cfg(test)]
mod tests_relation_actions;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_relations_policy;
#[cfg(test)]
mod tests_rename_attributes;
#[cfg(test)]
mod tests_rename_placement;
#[cfg(test)]
mod tests_reserved_keywords;
#[cfg(test)]
mod tests_server_only_placement;
#[cfg(test)]
mod tests_server_only_spelling;
#[cfg(test)]
mod tests_single_source_directives;
#[cfg(test)]
mod tests_snake_case_collisions;
#[cfg(test)]
mod tests_span_support;
#[cfg(test)]
mod tests_spatial;
#[cfg(test)]
mod tests_stream_attribute;
#[cfg(test)]
mod tests_transport;
#[cfg(test)]
mod tests_type_declaration_collisions;
#[cfg(test)]
mod tests_type_field_validators;
#[cfg(test)]
mod tests_types;
#[cfg(test)]
mod tests_unreachable_validators;
#[cfg(test)]
mod tests_validators;
#[cfg(test)]
mod tests_vector;
#[cfg(test)]
mod tests_version;
#[cfg(test)]
mod tests_view_from;
#[cfg(test)]
mod tests_views;

pub use diagnostics::SchemaError;
pub use diagnostics_related::RelatedSpan;
pub use directives::{FileDirectives, PathDirective};
pub use entry::{
    ANONYMOUS_SCHEMA, parse_schema, parse_schema_diagnostics, parse_schema_file,
    parse_schema_named, parse_schema_unvalidated,
};
pub use public_lists::{builtin_type_names, reserved_multi_file_keywords};
pub use validate::{FieldHost, field_attribute_names};

#[cfg(test)]
use relation_helpers::parse_relation_attribute;
