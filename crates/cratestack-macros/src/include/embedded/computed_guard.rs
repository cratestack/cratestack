//! `include_embedded_schema!` rejects any `@computed` field at expansion
//! time — embedded (rusqlite, sync, no policy enforcement) has no
//! response-composition boundary to resolve one against. Mirrors the
//! shape of `include::parse::reject_composite_primary_keys`: fail with
//! one clear `compile_error!` here rather than letting a computed field
//! silently vanish (it would today, since `generate_type_struct` /
//! `generate_model_struct_only` already exclude computed fields from the
//! server-side struct — see `docs/design/computed-fields.md` decision 3).

use proc_macro::TokenStream;
use syn::LitStr;

use crate::shared::is_computed_field;

/// Every `Owner.field` name (models first, then types, each in
/// declaration order) that carries `@computed`.
fn computed_field_names(schema: &cratestack_core::Schema) -> Vec<String> {
    schema
        .models
        .iter()
        .flat_map(|model| {
            model
                .fields
                .iter()
                .filter(|field| is_computed_field(field))
                .map(move |field| format!("{}.{}", model.name, field.name))
        })
        .chain(schema.types.iter().flat_map(|ty| {
            ty.fields
                .iter()
                .filter(|field| is_computed_field(field))
                .map(move |field| format!("{}.{}", ty.name, field.name))
        }))
        .collect()
}

pub(super) fn guard_embedded_no_computed_fields(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
) -> Result<(), TokenStream> {
    let offenders = computed_field_names(schema);
    if offenders.is_empty() {
        return Ok(());
    }
    Err(TokenStream::from(
        syn::Error::new(
            schema_path.span(),
            format!(
                "schema declares @computed fields ({}), which are response-composition \
                 resolvers; include_embedded_schema! has no response boundary — remove the \
                 computed fields or consume this schema through include_server_schema!/\
                 include_client_schema!",
                offenders.join(", "),
            ),
        )
        .to_compile_error(),
    ))
}

#[cfg(test)]
mod tests {
    use super::computed_field_names;

    #[test]
    fn flags_model_computed_field() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed
}
"#,
        )
        .expect("schema should parse");

        assert_eq!(computed_field_names(&schema), vec!["Image.proxyUrl"]);
    }

    #[test]
    fn flags_type_computed_field() {
        let schema = cratestack_parser::parse_schema(
            r#"
type Thumbnail {
  storageKey String
  url String @computed
}
"#,
        )
        .expect("schema should parse");

        assert_eq!(computed_field_names(&schema), vec!["Thumbnail.url"]);
    }

    #[test]
    fn does_not_flag_a_schema_without_computed_fields() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Account {
  id Int @id
}
"#,
        )
        .expect("schema should parse");

        assert!(computed_field_names(&schema).is_empty());
    }
}
