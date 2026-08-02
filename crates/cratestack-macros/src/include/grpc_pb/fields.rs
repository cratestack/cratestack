//! Field-selection predicates mirrored from `cratestack-proto::emit::mirror`
//! (`visible_model_fields`, `scalar_model_fields`, `model_allows_create`).
//! Reimplemented locally rather than depended on — `cratestack-proto` is a
//! separate crate emitting `.proto` text, this module emits Rust tokens
//! from the same source schema, and the two must not couple to each other
//! (the same "small pure helpers get reimplemented per crate" precedent
//! `cratestack-proto::emit::mirror`'s own module doc calls out). Keeping
//! the two implementations in sync is what
//! `crates/cratestack-proto/src/emit/tests_grpc.rs` and this crate's own
//! integration tests are for — a drift here would show up as a
//! `MissingLockEntry`-style `compile_error!` (wrong field set queried
//! against the committed lock), not a silent wire mismatch.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeDecl};

/// Fields on the generated `Model` pb message: everything except
/// `@server_only`. Relations stay in (rendered as message references by
/// `super::message::render_field`).
pub(crate) fn visible_model_fields(model: &Model) -> Vec<&Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

/// Fields on the generated `TypeDecl` pb message: everything except
/// `@server_only`.
pub(crate) fn visible_type_fields(ty: &TypeDecl) -> Vec<&Field> {
    ty.fields
        .iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

/// Base field set for `Create<M>Input`/`Update<M>Input`: relations and
/// `@server_only` fields excluded.
pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field) && !is_server_only_field(field))
        .collect()
}

fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Mirrors the create verb's policy gate — a model without at least one
/// `@@allow("create"|"all", ...)` rule fail-closes server-side, so no
/// `Create<M>Input` message exists for it (`cratestack-proto`'s
/// `emit::service` gates the gRPC `Create` method identically, on
/// `extra_messages.contains_key("Create<M>Input")`).
pub(crate) fn model_allows_create(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .filter_map(|attribute| allow_action(&attribute.raw))
        .any(|action| action == "create" || action == "all")
}

fn allow_action(raw: &str) -> Option<&str> {
    let inner = raw.trim().strip_prefix("@@allow(")?;
    let quote = inner.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &inner[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(source: &str) -> Model {
        cratestack_parser::parse_schema(source)
            .expect("schema should parse")
            .models
            .remove(0)
    }

    #[test]
    fn create_gate_requires_allow_create_or_all() {
        let none = model("model Widget {\n  id Int @id\n}\n");
        assert!(!model_allows_create(&none));

        let allow_create =
            model("model Widget {\n  id Int @id\n\n  @@allow(\"create\", true)\n}\n");
        assert!(model_allows_create(&allow_create));

        let allow_all = model("model Widget {\n  id Int @id\n\n  @@allow(\"all\", true)\n}\n");
        assert!(model_allows_create(&allow_all));

        let allow_read_only =
            model("model Widget {\n  id Int @id\n\n  @@allow(\"read\", true)\n}\n");
        assert!(!model_allows_create(&allow_read_only));
    }

    #[test]
    fn scalar_model_fields_excludes_relations_and_server_only() {
        let schema = cratestack_parser::parse_schema(
            r#"
model Author {
  id Int @id
}

model Post {
  id Int @id
  title String
  secret String @server_only
  authorId Int
  author Author @relation(fields:[authorId],references:[id])
}
"#,
        )
        .expect("schema should parse");
        let model_names: BTreeSet<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
        let post = schema.models.iter().find(|m| m.name == "Post").unwrap();
        let fields = scalar_model_fields(post, &model_names);
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["id", "title", "authorId"]);
    }
}
