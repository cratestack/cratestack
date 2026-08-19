pub(crate) fn dart_identifier(value: &str) -> String {
    if is_dart_keyword(value) {
        format!("{value}$")
    } else {
        value.to_owned()
    }
}

fn is_dart_keyword(value: &str) -> bool {
    matches!(
        value,
        "assert"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "extends"
            | "false"
            | "final"
            | "finally"
            | "for"
            | "if"
            | "in"
            | "is"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

pub(crate) fn to_camel_case(value: &str) -> String {
    let pascal = to_pascal_case(value);
    let mut chars = pascal.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_lowercase().collect::<String>() + chars.as_str()
}

/// Delegates to `cratestack_core::pascal_case::to_pascal_case` — see that
/// module doc for why this used to be its own `split_words`-based
/// implementation here and no longer is (cratestack-parser's
/// `builder_collisions` check needs the identical transform to predict
/// this crate's generated names at schema-parse time; two copies of the
/// same algorithm is exactly what drifted apart for `to_snake_case`
/// pre-#345).
pub(crate) fn to_pascal_case(value: &str) -> String {
    cratestack_core::pascal_case::to_pascal_case(value)
}

pub(crate) fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else if value.ends_with('y')
        && !matches!(
            value.chars().rev().nth(1),
            Some('a' | 'e' | 'i' | 'o' | 'u')
        )
    {
        format!("{}ies", &value[..value.len() - 1])
    } else {
        format!("{value}s")
    }
}

pub(crate) fn escape_dart_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
