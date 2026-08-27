//! The `decimal = RustDecimal | BigDecimal` argument accepted by all three
//! entry macros (cratestack#505 Direction 2 — see
//! `docs/design/decimal-backend-additivity.md` §7(b)).
//!
//! This is a schema-authored choice, not a Cargo feature: the whole point
//! of Direction 2 is that two independent crates, each invoking one of the
//! entry macros with a different `decimal = ...`, get the concrete type
//! they each asked for — a Cargo feature can't do that (cratestack#505 §5:
//! it would unify across the whole dependency graph, the exact bug this
//! fixes). Required only when the schema actually declares a `Decimal`
//! field somewhere ([`schema_uses_decimal`]); a schema with no `Decimal`
//! field needs no argument at all, preserving cratestack#521's "neither
//! backend selected, and nothing needs one" case.

use proc_macro::TokenStream;
use syn::LitStr;
use syn::parse::ParseStream;

use crate::shared::decimal_backend::DecimalBackend;

/// Parses a trailing `, decimal = RustDecimal` / `, decimal = BigDecimal`
/// if present, otherwise leaves `input` untouched and returns `None`.
/// Called after every other argument on a given entry macro has already
/// been consumed.
pub(super) fn parse_optional_decimal_arg(
    input: ParseStream<'_>,
) -> syn::Result<Option<DecimalBackend>> {
    if input.is_empty() {
        return Ok(None);
    }
    input.parse::<syn::Token![,]>()?;
    let key: syn::Ident = input.parse()?;
    if key != "decimal" {
        return Err(syn::Error::new(
            key.span(),
            "expected `decimal = RustDecimal` or `decimal = BigDecimal` \
             (only the `decimal` argument is recognised here)",
        ));
    }
    input.parse::<syn::Token![=]>()?;
    let value: syn::Ident = input.parse()?;
    match value.to_string().as_str() {
        "RustDecimal" => Ok(Some(DecimalBackend::RustDecimal)),
        "BigDecimal" => Ok(Some(DecimalBackend::BigDecimal)),
        other => Err(syn::Error::new(
            value.span(),
            format!("unsupported decimal backend `{other}`. supported: RustDecimal, BigDecimal"),
        )),
    }
}

/// Cross-checks the parsed `decimal = ...` argument (if any) against
/// whether the schema actually needs one. A schema with no `Decimal`
/// field anywhere compiles with `decimal` omitted (cratestack#521's
/// "neither" case, preserved). A schema WITH a `Decimal` field and no
/// `decimal = ...` argument is a compile error naming exactly what to
/// add, rather than silently guessing a backend (cratestack#505 §6a's
/// rejected precedence-resolution approach) or reaching for the ambient
/// Cargo feature this whole change exists to stop depending on.
pub(super) fn resolve_decimal_backend(
    schema_path: &LitStr,
    schema: &cratestack_core::Schema,
    given: Option<DecimalBackend>,
) -> Result<Option<DecimalBackend>, TokenStream> {
    if given.is_none() && schema_uses_decimal(schema) {
        return Err(TokenStream::from(
            syn::Error::new(
                schema_path.span(),
                "this schema declares a `Decimal` field, so this macro call needs a \
                 `decimal = RustDecimal` or `decimal = BigDecimal` argument \
                 (cratestack#505 — see docs/design/decimal-backend-additivity.md)",
            )
            .to_compile_error(),
        ));
    }
    Ok(given)
}

/// `true` if `"Decimal"` appears as a scalar type name anywhere in the
/// schema — model/mixin/type fields (including nested inside `Page<T>`/
/// `FindMany<T>`'s `generic_args`), and procedure arg/return types. Views
/// reuse `Field`, so their `Decimal` columns are covered by the same
/// model-field scan.
fn schema_uses_decimal(schema: &cratestack_core::Schema) -> bool {
    let fields = schema
        .models
        .iter()
        .flat_map(|m| m.fields.iter())
        .chain(schema.mixins.iter().flat_map(|m| m.fields.iter()))
        .chain(schema.types.iter().flat_map(|t| t.fields.iter()))
        .chain(schema.views.iter().flat_map(|v| v.fields.iter()));
    if fields.into_iter().any(|f| type_ref_uses_decimal(&f.ty)) {
        return true;
    }
    schema.procedures.iter().any(|p| {
        type_ref_uses_decimal(&p.return_type) || p.args.iter().any(|a| type_ref_uses_decimal(&a.ty))
    })
}

fn type_ref_uses_decimal(ty: &cratestack_core::TypeRef) -> bool {
    ty.name == "Decimal" || ty.generic_args.iter().any(type_ref_uses_decimal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> cratestack_core::Schema {
        cratestack_parser::parse_schema(source).expect("schema should parse")
    }

    #[test]
    fn detects_a_model_decimal_field() {
        let schema = parse(
            r#"
model Order {
  id Int @id
  total Decimal
}
"#,
        );
        assert!(schema_uses_decimal(&schema));
    }

    #[test]
    fn detects_a_decimal_field_nested_in_page() {
        let schema = parse(
            r#"
model Order {
  id Int @id
  total Decimal
}

procedure searchOrders(): Page<Order>
"#,
        );
        // Order's own field already trips it; this also exercises the
        // generic_args recursion via Page<Order> without needing a second
        // schema — Page<T>'s item is a model reference here, not Decimal
        // itself, so this mainly guards against a future false negative if
        // `Page<Decimal>` becomes syntactically valid.
        assert!(schema_uses_decimal(&schema));
    }

    #[test]
    fn no_decimal_field_anywhere_is_false() {
        let schema = parse(
            r#"
model Widget {
  id Int @id
  name String
}
"#,
        );
        assert!(!schema_uses_decimal(&schema));
    }

    #[test]
    fn detects_a_procedure_arg_decimal_type() {
        let schema = parse("procedure charge(amount: Decimal): Boolean");
        assert!(schema_uses_decimal(&schema));
    }
}
