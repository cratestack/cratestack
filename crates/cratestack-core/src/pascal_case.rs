//! Canonical PascalCase derivation for a schema-declared identifier
//! (currently: procedure names, which are declared camelCase in
//! `.cstack` source but need a PascalCase spelling wherever a generator
//! emits a top-level symbol derived from one — e.g. a procedure's
//! generated argument-wrapper class).
//!
//! Mirrors the `cratestack#345` precedent in [`crate::route_naming`]:
//! before this, `cratestack-client-dart::idents::to_pascal_case` and
//! `cratestack-client-typescript::naming::to_pascal_case` each carried
//! their own `split_words`-based implementation. Those two happened to
//! agree, but nothing enforced that, and `cratestack-parser` needed the
//! exact same transform to reserve the Dart-side generated name a
//! `builder_collisions` check has to predict *before* codegen runs —
//! three independent copies of the same algorithm is exactly the shape
//! that drifted apart for `to_snake_case`. One canonical implementation
//! here, reused by every consumer, closes that off structurally instead
//! of trusting three copies to stay byte-for-byte identical by hand.
//!
//! **Do not reimplement this.** Any code deriving a PascalCase symbol
//! name from a schema-declared identifier must call [`to_pascal_case`]
//! from here.

/// Splits `value` into words on `_`/`-`/` ` boundaries and ASCII
/// uppercase transitions (so `echoName` -> `["echo", "Name"]` and
/// `already_snake` -> `["already", "snake"]`), then re-joins with each
/// word's first character uppercased and the rest lowercased.
///
/// `echoName` -> `EchoName`, `already_snake` -> `AlreadySnake`,
/// `HTTPServer` -> `Httpserver` (consecutive uppercase letters are not
/// treated as separate words — this function optimizes for the
/// camelCase/snake_case identifiers `.cstack` schemas actually use, not
/// acronym-preserving casing).
pub fn to_pascal_case(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_procedure_name() {
        assert_eq!(to_pascal_case("echoName"), "EchoName");
    }

    #[test]
    fn snake_case_input() {
        assert_eq!(to_pascal_case("already_snake"), "AlreadySnake");
    }

    #[test]
    fn hyphen_and_space_separators() {
        assert_eq!(to_pascal_case("kebab-case-name"), "KebabCaseName");
        assert_eq!(to_pascal_case("space case name"), "SpaceCaseName");
    }

    #[test]
    fn already_pascal_case_is_unchanged() {
        assert_eq!(to_pascal_case("EchoName"), "EchoName");
    }

    #[test]
    fn empty_string() {
        assert_eq!(to_pascal_case(""), "");
    }
}
