use super::*;
use crate::SourceSpan;
use crate::schema::model::EnumVariant;

const SPAN: SourceSpan = SourceSpan {
    start: 0,
    end: 0,
    line: 1,
};

fn ty(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: SPAN,
        arity,
        generic_args: Vec::new(),
        int_args: Vec::new(),
        ident_args: Vec::new(),
    }
}

fn required(name: &str) -> TypeRef {
    ty(name, TypeArity::Required)
}

fn status(ordinals: Option<[i64; 2]>) -> EnumDecl {
    let variant = |name: &str, ordinal: Option<i64>| EnumVariant {
        docs: Vec::new(),
        name: name.to_owned(),
        ordinal,
        span: SPAN,
    };
    EnumDecl {
        docs: Vec::new(),
        name: "Status".to_owned(),
        name_span: SPAN,
        variants: vec![
            variant("Draft", ordinals.map(|o| o[0])),
            variant("Live", ordinals.map(|o| o[1])),
        ],
        reserved: None,
        span: SPAN,
    }
}

fn accepted(inner: &str, name: &str) -> LiteralDefault {
    literal_default(inner, &required(name), None).unwrap_or_else(|e| panic!("{inner}: {e}"))
}

fn refused(inner: &str, name: &str) -> String {
    literal_default(inner, &required(name), None).expect_err(inner)
}

#[test]
fn scalars_parse_to_their_value_and_sql() {
    let cases = [
        ("false", "Boolean", LiteralValue::Bool(false), "false"),
        ("TRUE", "Boolean", LiteralValue::Bool(true), "true"),
        ("-7", "Int", LiteralValue::Int(-7), "-7"),
        (
            "9223372036854775807",
            "BigInt",
            LiteralValue::Int(i64::MAX),
            "9223372036854775807",
        ),
        ("1.5e3", "Float", LiteralValue::Float(1500.0), "1.5e3"),
        ("0", "Float", LiteralValue::Float(0.0), "0"),
        (
            "-12.50",
            "Decimal",
            LiteralValue::Decimal("-12.50".into()),
            "-12.50",
        ),
        ("'3'", "Decimal", LiteralValue::Decimal("3".into()), "'3'"),
        (
            "\"draft\"",
            "String",
            LiteralValue::Text("draft".into()),
            "'draft'",
        ),
    ];
    for (inner, name, value, sql) in &cases {
        let literal = accepted(inner, name);
        assert_eq!(&literal.value, value, "{inner}");
        assert_eq!(literal.sql, *sql, "{inner}");
    }
}

#[test]
fn strings_unescape_and_render_single_quoted() {
    let literal = accepted(r#""say \"hi\", it's""#, "String");
    assert_eq!(
        literal.value,
        LiteralValue::Text(r#"say "hi", it's"#.into())
    );
    assert_eq!(literal.sql, r#"'say "hi", it''s'"#);
    assert_eq!(accepted("'a@b.c'", "String").sql, "'a@b.c'");
}

#[test]
fn text_like_types_check_their_shape() {
    let id = "6f1c2d4e-8b3a-4c5d-9e7f-0a1b2c3d4e5f";
    assert_eq!(
        accepted(&format!("\"{id}\""), "Uuid").value,
        LiteralValue::Uuid(Uuid::parse_str(id).unwrap())
    );
    assert!(matches!(
        accepted("'2026-01-02T03:04:05Z'", "DateTime").value,
        LiteralValue::DateTime(_)
    ));
    assert!(refused("'not-a-uuid'", "Uuid").contains("not a UUID"));
    assert!(refused("'yesterday'", "DateTime").contains("RFC 3339"));
    assert!(refused("'Not A Cuid'", "Cuid").contains("not a cuid"));
}

#[test]
fn literals_that_do_not_fit_are_refused() {
    for (inner, name, says) in [
        ("yes", "Boolean", "`true` or `false`"),
        ("1.5", "Int", "decimal integer"),
        ("9223372036854775808", "BigInt", "decimal integer"),
        ("'1'", "Int", "decimal integer"),
        ("NaN", "Float", "finite"),
        ("inf", "Float", "finite"),
        ("1e999", "Float", "finite"),
        ("1.2.3", "Decimal", "plain decimal"),
        ("draft", "String", "write `@default(\"draft\")`"),
        ("CURRENT_TIMESTAMP", "DateTime", "`@default(now())`"),
        ("'x'", "Json", "takes no literal default"),
        ("null", "String", "already NULL"),
    ] {
        let message = refused(inner, name);
        assert!(message.contains(says), "{inner} on {name}: {message}");
    }
    let list = literal_default("\"a\"", &ty("String", TypeArity::List), None).unwrap_err();
    assert!(list.contains("list field"), "{list}");
}

#[test]
fn enum_variants_are_bare_or_quoted() {
    let decl = status(None);
    let ty = required("Status");
    for inner in ["Draft", "'Draft'", "\"Draft\""] {
        let literal = literal_default(inner, &ty, Some(&decl)).unwrap();
        assert_eq!(literal.sql, "'Draft'", "{inner}");
    }
    let message = literal_default("Archived", &ty, Some(&decl)).unwrap_err();
    assert!(message.contains("variants: Draft, Live"), "{message}");
}

#[test]
fn ordinal_enum_defaults_are_their_number() {
    let decl = status(Some([1, 2]));
    let literal = literal_default("Live", &required("Status"), Some(&decl)).unwrap();
    assert_eq!(literal.sql, "2");
    assert_eq!(
        literal.value,
        LiteralValue::Enum {
            variant: "Live".into(),
            ordinal: Some(2)
        }
    );
}
