use chumsky::prelude::*;
use cratestack_core::{SourceSpan, TypeArity, TypeRef};

use crate::diagnostics::SchemaError;
use crate::line_helpers::Line;

/// One entry in a parametric type's parenthesized argument list —
/// either a compile-time integer (`Vector(1536)`'s dimension,
/// `Geography(Polygon, 4326)`'s SRID) or a bare identifier
/// (`Geography(Polygon, …)`'s geometry subtype).
#[derive(Clone, Copy)]
enum ParametricArg<'a> {
    Int(&'a str),
    Ident(&'a str),
}

pub(super) fn parse_type_ref(
    raw: &str,
    line: &Line<'_>,
    raw_offset: usize,
) -> Result<TypeRef, SchemaError> {
    // Parametric scalars (see `docs/design/extensions.md` §6/§6b): an
    // identifier optionally followed by a parenthesized, comma-separated
    // argument list, then the usual arity suffix. Two shapes use this
    // today — `Vector(1536)` (one integer) and `Geography(Polygon, 4326)`
    // (one bare identifier plus an optional integer SRID, cratestack#842).
    //
    // The argument list is deliberately generic in the grammar: any
    // identifier may carry any mix of int/ident arguments here, and
    // `validate_type_ref` in `cratestack-parser::validate` is what
    // actually restricts which types accept which arguments. Keeping the
    // restriction in validation rather than the grammar is what lets an
    // unrecognised parametric type produce a precise "type `X` does not
    // accept a parametric argument" diagnostic instead of a generic
    // "invalid type reference".
    let argument = choice((
        text::int::<_, extra::Err<Simple<char>>>(10).map(ParametricArg::Int),
        text::ident::<_, extra::Err<Simple<char>>>().map(ParametricArg::Ident),
    ))
    .padded();

    let parser = text::ident::<_, extra::Err<Simple<char>>>()
        .then(
            just('(')
                .ignore_then(
                    argument
                        .separated_by(just(','))
                        .at_least(1)
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just(')'))
                .or_not(),
        )
        .then(choice((
            just("[]").to(TypeArity::List),
            just("?").to(TypeArity::Optional),
            end().to(TypeArity::Required),
        )))
        .then_ignore(end());

    parser
        .parse(raw)
        .into_result()
        .ok()
        .and_then(|((name, args), arity)| {
            let mut int_args = Vec::new();
            let mut ident_args = Vec::new();
            for arg in args.unwrap_or_default() {
                match arg {
                    ParametricArg::Int(digits) => int_args.push(digits.parse::<u32>().ok()?),
                    ParametricArg::Ident(ident) => ident_args.push(ident.to_owned()),
                }
            }
            Some(TypeRef {
                name: name.to_owned(),
                name_span: SourceSpan {
                    start: line.start + raw_offset,
                    end: line.start + raw_offset + name.len(),
                    line: line.number,
                },
                arity,
                generic_args: Vec::new(),
                int_args,
                ident_args,
            })
        })
        .ok_or(())
        .or_else(|()| parse_builtin_generic_type_ref(raw, line, raw_offset))
        .map_err(|()| {
            SchemaError::new(
                format!("invalid type reference: {raw}"),
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })
}

/// Built-in generic type names — every one of these accepts exactly one
/// `<T>` argument in this grammar today (`Page<T>` for procedure return
/// types, `FindMany<T>` for procedure arguments). Add new entries here as
/// new generic builtins are introduced; the parsing logic below is
/// otherwise fully shared.
const GENERIC_BUILTIN_NAMES: &[&str] = &["Page", "FindMany"];

fn parse_builtin_generic_type_ref(
    raw: &str,
    line: &Line<'_>,
    raw_offset: usize,
) -> Result<TypeRef, ()> {
    let (base, arity) = if let Some(base) = raw.strip_suffix("[]") {
        (base.trim(), TypeArity::List)
    } else if let Some(base) = raw.strip_suffix('?') {
        (base.trim(), TypeArity::Optional)
    } else {
        (raw.trim(), TypeArity::Required)
    };

    let name = GENERIC_BUILTIN_NAMES
        .iter()
        .copied()
        .find(|name| {
            base.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('<'))
        })
        .ok_or(())?;

    let inner = base[name.len() + 1..].strip_suffix('>').ok_or(())?;

    let inner_offset = base.find('<').ok_or(())? + 1;
    let inner_ref =
        parse_type_ref(inner.trim(), line, raw_offset + inner_offset).map_err(|_| ())?;
    Ok(TypeRef {
        name: name.to_owned(),
        name_span: SourceSpan {
            start: line.start + raw_offset,
            end: line.start + raw_offset + name.len(),
            line: line.number,
        },
        arity,
        generic_args: vec![inner_ref],
        int_args: Vec::new(),
        ident_args: Vec::new(),
    })
}
