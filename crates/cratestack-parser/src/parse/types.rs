use chumsky::prelude::*;
use cratestack_core::{SourceSpan, TypeArity, TypeRef};

use crate::diagnostics::SchemaError;
use crate::line_helpers::Line;

pub(super) fn parse_type_ref(
    raw: &str,
    line: &Line<'_>,
    raw_offset: usize,
) -> Result<TypeRef, SchemaError> {
    let parser = text::ident::<_, extra::Err<Simple<char>>>()
        .then(choice((
            just("[]").to(TypeArity::List),
            just("?").to(TypeArity::Optional),
            end().to(TypeArity::Required),
        )))
        .then_ignore(end());

    parser
        .parse(raw)
        .into_result()
        .map(|(name, arity)| TypeRef {
            name: name.to_owned(),
            name_span: SourceSpan {
                start: line.start + raw_offset,
                end: line.start + raw_offset + name.len(),
                line: line.number,
            },
            arity,
            generic_args: Vec::new(),
        })
        .or_else(|_| parse_builtin_generic_type_ref(raw, line, raw_offset))
        .map_err(|_| {
            SchemaError::new(
                format!("invalid type reference: {raw}"),
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })
}

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

    let Some(inner) = base
        .strip_prefix("Page<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Err(());
    };

    let inner_offset = base.find('<').ok_or(())? + 1;
    let inner = parse_type_ref(inner.trim(), line, raw_offset + inner_offset).map_err(|_| ())?;
    Ok(TypeRef {
        name: "Page".to_owned(),
        name_span: SourceSpan {
            start: line.start + raw_offset,
            end: line.start + raw_offset + "Page".len(),
            line: line.number,
        },
        arity,
        generic_args: vec![inner],
    })
}
