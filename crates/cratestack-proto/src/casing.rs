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

#[cfg(test)]
mod tests {
    use super::to_screaming_snake_case;

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
}
