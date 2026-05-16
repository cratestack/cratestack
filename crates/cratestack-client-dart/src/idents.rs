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

pub(crate) fn to_pascal_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        })
        .collect::<String>()
}

pub(crate) fn to_snake_case(value: &str) -> String {
    split_words(value)
        .into_iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            continue;
        }

        if ch.is_ascii_uppercase() && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }

        current.push(ch);
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
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
