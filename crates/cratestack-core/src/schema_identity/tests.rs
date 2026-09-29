use super::{normalize_attribute_text, schema_digest, schema_digest_hex};
use crate::schema::{
    Attribute, EnumDecl, EnumVariant, Field, Model, Schema, SourceSpan, TypeArity, TypeRef,
};

fn span(n: usize) -> SourceSpan {
    SourceSpan {
        start: n,
        end: n + 1,
        line: n,
    }
}

fn attr(raw: &str, at: usize) -> Attribute {
    Attribute {
        raw: raw.to_owned(),
        span: span(at),
    }
}

fn ty(name: &str) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: span(0),
        arity: TypeArity::Required,
        generic_args: vec![],
        int_args: vec![],
        ident_args: vec![],
    }
}

fn field(name: &str, ty_name: &str, attrs: Vec<Attribute>, at: usize) -> Field {
    Field {
        docs: vec![format!("doc {at}")],
        name: name.to_owned(),
        name_span: span(at),
        ty: ty(ty_name),
        attributes: attrs,
        span: span(at),
    }
}

fn model(name: &str, fields: Vec<Field>, attributes: Vec<Attribute>, at: usize) -> Model {
    Model {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(at),
        fields,
        attributes,
        span: span(at),
        mcp: None,
    }
}

fn enum_decl(name: &str, variants: &[&str]) -> EnumDecl {
    EnumDecl {
        docs: vec![],
        name: name.to_owned(),
        name_span: span(0),
        variants: variants
            .iter()
            .map(|v| EnumVariant {
                docs: vec![],
                name: (*v).to_owned(),
                span: span(0),
            })
            .collect(),
        span: span(0),
    }
}

fn empty() -> Schema {
    serde_json::from_str(
        r#"{"datasource":null,"auth":null,"config_blocks":[],"mixins":[],
            "models":[],"types":[],"enums":[],"procedures":[]}"#,
    )
    .unwrap()
}

/// `model Widget { id Int @id }`, the parser's IR for it.
pub(super) fn widget(at: usize) -> Schema {
    let mut schema = empty();
    schema.models.push(model(
        "Widget",
        vec![field("id", "Int", vec![attr("@id", at)], at)],
        vec![],
        at,
    ));
    schema
}

#[test]
fn golden_digest_of_a_one_model_schema() {
    assert_eq!(schema_digest_hex(&widget(0)), GOLDEN_WIDGET_HEX);
}

/// The digest of `model Widget { id Int @id }`. Every other crate that
/// hashes a schema asserts this same value (cratestack#1065).
pub(crate) const GOLDEN_WIDGET_HEX: &str =
    "95c11ca292e854994d452dcc0d88c7de6ab309b0422fc1e30ab46e56a7757a5f";

#[test]
fn spans_and_docs_do_not_move_the_digest() {
    assert_eq!(schema_digest(&widget(0)), schema_digest(&widget(40)));
}

#[test]
fn top_level_declaration_order_does_not_move_the_digest() {
    let (a, b) = (model("A", vec![], vec![], 1), model("B", vec![], vec![], 2));
    let mut one = empty();
    one.models = vec![a.clone(), b.clone()];
    let mut two = empty();
    two.models = vec![b, a];
    assert_eq!(schema_digest(&one), schema_digest(&two));
}

#[test]
fn field_order_does_not_move_the_digest() {
    let (x, y) = (
        field("x", "Int", vec![], 1),
        field("y", "String", vec![], 2),
    );
    let mut one = empty();
    one.models = vec![model("M", vec![x.clone(), y.clone()], vec![], 0)];
    let mut two = empty();
    two.models = vec![model("M", vec![y, x], vec![], 0)];
    assert_eq!(schema_digest(&one), schema_digest(&two));
}

#[test]
fn enum_variant_order_moves_the_digest() {
    let mut one = empty();
    one.enums = vec![enum_decl("Role", &["Admin", "User"])];
    let mut two = empty();
    two.enums = vec![enum_decl("Role", &["User", "Admin"])];
    assert_ne!(schema_digest(&one), schema_digest(&two));
}

#[test]
fn attribute_whitespace_outside_strings_does_not_move_the_digest() {
    let with = |raw: &str| {
        let mut schema = empty();
        schema.models = vec![model("M", vec![], vec![attr(raw, 0)], 0)];
        schema_digest(&schema)
    };
    assert_eq!(
        with("@@allow( \"read\" , auth() != null )"),
        with("@@allow(\"read\",auth()!=null)")
    );
}

#[test]
fn whitespace_inside_a_sql_string_moves_the_digest() {
    let with = |raw: &str| {
        let mut schema = empty();
        schema.models = vec![model("M", vec![], vec![attr(raw, 0)], 0)];
        schema_digest(&schema)
    };
    assert_ne!(with(r#"@@sql("SELECT  1")"#), with(r#"@@sql("SELECT 1")"#));
}

#[test]
fn normaliser_keeps_literals_and_separates_words() {
    assert_eq!(
        normalize_attribute_text("@default( false )"),
        "@default(false)"
    );
    assert_eq!(
        normalize_attribute_text("@@allow(x  in\n  y)"),
        "@@allow(x in y)"
    );
    assert_eq!(
        normalize_attribute_text(r#"@regex( "a  \" b" )"#),
        r#"@regex("a  \" b")"#
    );
    assert_eq!(
        normalize_attribute_text("@@sql( \"\"\"\n  SELECT  \"x\"  \n\"\"\" )"),
        "@@sql(\"\"\"\n  SELECT  \"x\"  \n\"\"\")"
    );
}
