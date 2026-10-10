//! Fixtures shared by the codegen tests that guard against a built-in
//! scalar falling through a catch-all arm (ADR 0019, PR B).
//!
//! Every generator that maps a scalar by name has a catch-all, and a catch-all
//! is where `BigInt` first went wrong: it compiled, and did the wrong thing
//! silently (`Value::Null`, no `@range` check, no `Where` entry). A guard
//! iterates [`builtin_scalars`], which is read from the parser's own list, so
//! a scalar added to the language later is covered here without anyone
//! remembering to extend a test.

use cratestack_core::{Field, SourceSpan, TypeArity, TypeRef};

pub(crate) fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

pub(crate) fn type_ref(name: &str, arity: TypeArity) -> TypeRef {
    TypeRef {
        name: name.to_owned(),
        name_span: span(),
        arity,
        generic_args: Vec::new(),
        // `Vector` is the one parametric scalar; its dimension never reaches
        // the token mapping, so any value serves.
        int_args: if name == "Vector" {
            vec![3]
        } else {
            Vec::new()
        },
        ident_args: Vec::new(),
    }
}

pub(crate) fn field(name: &str, ty: &str, arity: TypeArity) -> Field {
    Field {
        docs: Vec::new(),
        name: name.to_owned(),
        name_span: span(),
        ty: type_ref(ty, arity),
        attributes: Vec::new(),
        span: span(),
    }
}

/// The parser's built-in type names that are scalars: everything it lists
/// except the generic wrappers, which are not a field type on their own.
pub(crate) fn builtin_scalars() -> Vec<&'static str> {
    cratestack_parser::builtin_type_names()
        .iter()
        .copied()
        .filter(|name| !matches!(*name, "Page" | "PageInput" | "FindMany"))
        .collect()
}

/// Runs `f` with a `Decimal` backend selected, as every entry macro does before
/// it generates anything: `Decimal` names a concrete type, so a guard that
/// walks every built-in scalar needs the scope or it aborts on `Decimal`.
pub(crate) fn with_rust_decimal<R>(f: impl FnOnce() -> R) -> R {
    use super::decimal_backend::{DecimalBackend, with_decimal_backend};
    with_decimal_backend(Some(DecimalBackend::RustDecimal), f)
}

/// Compares token streams as text, which is how the existing generator tests
/// do it: `quote!` renders every punctuation token with spaces around it.
pub(crate) fn render(tokens: &proc_macro2::TokenStream) -> String {
    tokens.to_string()
}

#[test]
fn the_catalog_names_bigint_and_the_other_scalars() {
    let scalars = builtin_scalars();
    for expected in [
        "String",
        "Cuid",
        "Int",
        "BigInt",
        "Float",
        "Boolean",
        "DateTime",
        "Decimal",
        "Json",
        "Bytes",
        "Uuid",
        "Vector",
        "Geography",
        "Geometry",
    ] {
        assert!(
            scalars.contains(&expected),
            "the parser no longer lists `{expected}`; the guards below would stop covering it: {scalars:?}"
        );
    }
    for wrapper in ["Page", "PageInput", "FindMany"] {
        assert!(!scalars.contains(&wrapper));
    }
}
