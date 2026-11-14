//! The type lookups and field checks the leg shapes share. Matching is by
//! schema field name, which is the wire name.

use cratestack_core::{Field, Schema, TypeArity, TypeDecl, TypeRef};

use super::legs::Leg;
use crate::diagnostics::SchemaError;

/// Which side of the procedure a field is on, for the messages.
#[derive(Clone, Copy)]
pub(super) enum Side {
    Argument,
    Return,
}

impl Side {
    fn text(self) -> &'static str {
        match self {
            Self::Argument => "argument",
            Self::Return => "return",
        }
    }
}

/// What a field must be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Want {
    Text,
    OptionalText,
    Key,
    OptionalKey,
}

impl Want {
    fn describe(self) -> &'static str {
        match self {
            Self::Text => "a required `String`",
            Self::OptionalText => "an optional `String?`",
            Self::Key => "a required JWK `type` (`kty`, `crv`, `x`, `y`, each a `String`)",
            Self::OptionalKey => {
                "an optional JWK `type?` (`kty`, `crv`, `x`, `y`, each a `String`)"
            }
        }
    }
}

/// The `type` a plain (required, non-generic) type reference names.
fn declared<'a>(schema: &'a Schema, ty: &TypeRef) -> Option<&'a TypeDecl> {
    (ty.arity == TypeArity::Required && ty.generic_args.is_empty())
        .then(|| schema.types.iter().find(|decl| decl.name == ty.name))
        .flatten()
}

/// The one argument's `type`: a `type` declaration, not a list, not optional.
pub(super) fn argument<'a>(schema: &'a Schema, leg: &Leg<'a>) -> Result<&'a TypeDecl, SchemaError> {
    let [arg] = leg.procedure.args.as_slice() else {
        return Err(leg.fail(format!(
            "takes exactly one argument, a `type`; this has {}",
            leg.procedure.args.len()
        )));
    };
    declared(schema, &arg.ty).ok_or_else(|| {
        leg.fail(format!(
            "its argument `{}` must be a `type` declaration (not a model, list, optional or \
             scalar), not `{}`",
            arg.name, arg.ty.name
        ))
    })
}

/// The return `type`, under the same rule.
pub(super) fn returns<'a>(schema: &'a Schema, leg: &Leg<'a>) -> Result<&'a TypeDecl, SchemaError> {
    declared(schema, &leg.procedure.return_type).ok_or_else(|| {
        leg.fail(format!(
            "it must return a `type` declaration (not a model, list, optional or scalar), not \
             `{}`",
            leg.procedure.return_type.name
        ))
    })
}

pub(super) fn field<'a>(ty: &'a TypeDecl, name: &str) -> Option<&'a Field> {
    ty.fields.iter().find(|field| field.name == name)
}

fn is_text(ty: &TypeRef, arity: TypeArity) -> bool {
    ty.name == "String" && ty.arity == arity && ty.generic_args.is_empty()
}

/// A JWK is a `type` of exactly `kty`, `crv`, `x`, `y`, each a required `String`.
fn is_key(schema: &Schema, ty: &TypeRef, arity: TypeArity) -> bool {
    if ty.arity != arity || !ty.generic_args.is_empty() {
        return false;
    }
    let Some(decl) = schema.types.iter().find(|decl| decl.name == ty.name) else {
        return false;
    };
    let mut names: Vec<&str> = decl.fields.iter().map(|f| f.name.as_str()).collect();
    names.sort_unstable();
    names == ["crv", "kty", "x", "y"]
        && decl
            .fields
            .iter()
            .all(|f| is_text(&f.ty, TypeArity::Required))
}

pub(super) fn matches(schema: &Schema, field: &Field, want: Want) -> bool {
    match want {
        Want::Text => is_text(&field.ty, TypeArity::Required),
        Want::OptionalText => is_text(&field.ty, TypeArity::Optional),
        Want::Key => is_key(schema, &field.ty, TypeArity::Required),
        Want::OptionalKey => is_key(schema, &field.ty, TypeArity::Optional),
    }
}

/// `ty` must have `name`, shaped `want`.
pub(super) fn require(
    schema: &Schema,
    leg: &Leg<'_>,
    (ty, side): (&TypeDecl, Side),
    name: &str,
    want: Want,
) -> Result<(), SchemaError> {
    match field(ty, name) {
        None => Err(leg.fail(format!(
            "its {} type `{}` has no `{name}`; it needs {}",
            side.text(),
            ty.name,
            want.describe()
        ))),
        Some(found) if !matches(schema, found, want) => Err(leg.fail(format!(
            "`{name}` of its {} type `{}` must be {}, not `{}`{}",
            side.text(),
            ty.name,
            want.describe(),
            found.ty.name,
            match found.ty.arity {
                TypeArity::Required => "",
                TypeArity::Optional => "?",
                TypeArity::List => "[]",
            }
        ))),
        Some(_) => Ok(()),
    }
}

/// `userPresent` is on a leg's type exactly when the enrolment has one: all
/// four enrolment shapes or none, or a challenge or attestation would be
/// dropped silently.
pub(super) fn user_present_consistent(
    schema: &Schema,
    leg: &Leg<'_>,
    (ty, side): (&TypeDecl, Side),
    expected: bool,
    want: Want,
) -> Result<(), SchemaError> {
    match (expected, field(ty, "userPresent")) {
        (true, _) => require(schema, leg, (ty, side), "userPresent", want),
        (false, None) => Ok(()),
        (false, Some(_)) => Err(leg.fail(format!(
            "its {} type `{}` has `userPresent`, but `beginEnrolment`'s argument has none: the \
             user-present key is in all four enrolment shapes or in none",
            side.text(),
            ty.name
        ))),
    }
}

/// Fields of `ty` other than `known`: the app's own, which a leg carries
/// through `EnrolmentStart.fields`, `data` or `claims`. Only `String` and
/// `String?` fit those maps.
pub(super) fn app_fields<'a>(
    leg: &Leg<'_>,
    (ty, side): (&'a TypeDecl, Side),
    known: &[&str],
) -> Result<Vec<&'a Field>, SchemaError> {
    let mut fields = Vec::new();
    for field in ty
        .fields
        .iter()
        .filter(|f| !known.contains(&f.name.as_str()))
    {
        let text =
            is_text(&field.ty, TypeArity::Required) || is_text(&field.ty, TypeArity::Optional);
        if !text {
            return Err(leg.fail(format!(
                "`{}` of its {} type `{}` is the app's own field, which is carried as text: it \
                 must be `String` or `String?`, not `{}`",
                field.name,
                side.text(),
                ty.name,
                field.ty.name
            )));
        }
        fields.push(field);
    }
    Ok(fields)
}
