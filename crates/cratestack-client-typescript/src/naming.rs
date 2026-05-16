use std::collections::BTreeSet;

use cratestack_core::{Procedure, Schema};

pub(crate) fn occupied_type_names(schema: &Schema) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for ty in &schema.types {
        names.insert(ty.name.clone());
    }
    for enum_decl in &schema.enums {
        names.insert(enum_decl.name.clone());
    }
    for model in &schema.models {
        names.insert(model.name.clone());
        names.insert(format!("Create{}Input", model.name));
        names.insert(format!("Update{}Input", model.name));
    }
    names
}

pub(crate) fn procedure_wrapper_name(
    procedure: &Procedure,
    occupied_type_names: &BTreeSet<String>,
) -> String {
    let base = format!("{}Args", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&base) {
        return base;
    }

    let procedure_name = format!("{}ProcedureArgs", to_pascal_case(&procedure.name));
    if !occupied_type_names.contains(&procedure_name) {
        return procedure_name;
    }

    format!("{}ProcedureRequest", to_pascal_case(&procedure.name))
}

pub(crate) fn ts_identifier(value: &str) -> String {
    if is_ts_keyword(value) {
        format!("{value}_")
    } else {
        value.to_owned()
    }
}

fn is_ts_keyword(value: &str) -> bool {
    matches!(
        value,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
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

pub(crate) fn package_class_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect()
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

pub(crate) fn escape_ts_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
