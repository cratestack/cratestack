//! `.cstack` enum names are PascalCase; proto3's required zero-value variant
//! follows `<SCREAMING_ENUM_NAME>_UNSPECIFIED`. `cratestack-macros::shared`
//! has a PascalCase/camelCase -> snake_case helper already, but this crate
//! must not depend on `cratestack-macros` (macros depends on parser/core,
//! not the other way around — see `docs/design/protobuf.md` §3.3 and the
//! crate-layering rule in the repo's `CLAUDE.md`), so the transform is
//! re-implemented locally rather than reused.

pub(crate) fn to_screaming_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_uppercase() && index > 0 {
            output.push('_');
        }
        output.extend(ch.to_uppercase());
    }
    output
}

/// `.cstack` procedure names are camelCase (`publishPost`); synthesized
/// message names (`<Procedure>Input`/`Output`) need the PascalCase form.
/// `cratestack-client-typescript::naming::to_pascal_case` does the same
/// job — reimplemented here rather than depended on, same crate-layering
/// rule as [`to_screaming_snake_case`] above.
pub(crate) fn to_pascal_case(value: &str) -> String {
    let mut output = String::new();
    let mut capitalize_next = true;
    for ch in value.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            output.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{to_pascal_case, to_screaming_snake_case};

    #[test]
    fn converts_pascal_case() {
        assert_eq!(to_screaming_snake_case("OrderStatus"), "ORDER_STATUS");
    }

    #[test]
    fn leaves_single_word_uppercased() {
        assert_eq!(to_screaming_snake_case("Order"), "ORDER");
    }

    #[test]
    fn handles_consecutive_capitals() {
        assert_eq!(to_screaming_snake_case("HTTPStatus"), "H_T_T_P_STATUS");
    }

    #[test]
    fn pascal_cases_camel_case_procedure_name() {
        assert_eq!(to_pascal_case("publishPost"), "PublishPost");
    }

    #[test]
    fn pascal_case_leaves_already_pascal_name_alone() {
        assert_eq!(to_pascal_case("GetFeed"), "GetFeed");
    }
}
