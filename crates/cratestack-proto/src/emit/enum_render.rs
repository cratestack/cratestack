//! Renders an `enum { ... }` block using the numbers `build_lock` already
//! assigned — never re-derived here, per ticket #169's Part C.

use cratestack_core::EnumDecl;

use super::error::ProtoEmitError;
use crate::EnumLock;
use crate::casing::to_screaming_snake_case;

pub(super) fn render_enum(decl: &EnumDecl, lock: &EnumLock) -> Result<String, ProtoEmitError> {
    // Declared variant identifiers are emitted exactly as written in the
    // schema, not case-transformed to SCREAMING_SNAKE: `EnumLock.variants`
    // is keyed by the raw declared name (see `lock::assign::build_enum_lock`
    // — only the synthetic zero value is computed via
    // `to_screaming_snake_case`), and a lookup key that doesn't match the
    // lock's own key would be a bug, not a style choice.
    let unspecified = format!("{}_UNSPECIFIED", to_screaming_snake_case(&decl.name));
    let zero = *lock
        .variants
        .get(&unspecified)
        .ok_or_else(|| ProtoEmitError::MissingEnumLock(decl.name.clone()))?;

    let mut text = format!("enum {} {{\n", decl.name);
    text.push_str(&format!("  {unspecified} = {zero};\n"));
    for variant in &decl.variants {
        let number =
            *lock
                .variants
                .get(&variant.name)
                .ok_or_else(|| ProtoEmitError::MissingLockEntry {
                    owner: decl.name.clone(),
                    field: variant.name.clone(),
                })?;
        text.push_str(&format!("  {} = {number};\n", variant.name));
    }
    text.push_str("}\n");
    Ok(text)
}
