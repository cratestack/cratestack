//! Name-level matching on a field attribute's raw text (cratestack#1074).
//!
//! [`crate::schema::model::Attribute`] keeps the attribute as written, and each
//! consumer used to match that text itself. The matches drifted: most sites
//! tested `raw.starts_with("@id")`, which also took `@identity`, `@idx` and
//! `@id_foo` for the primary key, while `cratestack-migrate` matched exactly
//! — so migrate and codegen could disagree on which column is the key.
//! Every "is this the primary key" question now goes through
//! [`is_primary_key_attribute`] instead.
//!
//! `@id` takes no arguments: only the bare spelling is the primary key.
//! `cratestack-parser` rejects `@id(...)` outright, so a written `@id(...)`
//! never reaches a consumer that would read it as "not a key".

use crate::schema::model::{Attribute, Field};

/// The one spelling that marks a field as its model's (or view's) primary
/// key.
pub const PRIMARY_KEY_ATTRIBUTE: &str = "@id";

/// The bare name of a field-level attribute, with the `@` sigil and any
/// `(...)` argument list removed: `@length(min: 1)` -> `Some("length")`.
/// `None` for model-level `@@...` text or text without the sigil.
pub fn field_attribute_name(raw: &str) -> Option<&str> {
    if raw.starts_with("@@") {
        return None;
    }
    let without_sigil = raw.strip_prefix('@')?;
    Some(match without_sigil.find('(') {
        Some(open) => &without_sigil[..open],
        None => without_sigil,
    })
}

/// Whether `attribute` marks its field as the primary key. Exact match on
/// [`PRIMARY_KEY_ATTRIBUTE`]: `@identity`, `@idx` and `@id_foo` are other
/// (unknown) attributes, not a primary key.
pub fn is_primary_key_attribute(attribute: &Attribute) -> bool {
    attribute.raw == PRIMARY_KEY_ATTRIBUTE
}

/// Whether `attribute` is a `@relation` / `@relation(...)` declaration —
/// by name, so a bare `@relation` counts too. `@relationship` does not.
pub fn is_relation_attribute(attribute: &Attribute) -> bool {
    field_attribute_name(&attribute.raw) == Some("relation")
}

impl Field {
    /// Whether this field carries the primary-key attribute; see
    /// [`is_primary_key_attribute`].
    pub fn is_primary_key(&self) -> bool {
        self.attributes.iter().any(is_primary_key_attribute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SourceSpan;

    fn attribute(raw: &str) -> Attribute {
        Attribute {
            raw: raw.to_owned(),
            span: SourceSpan {
                start: 0,
                end: raw.len(),
                line: 1,
            },
        }
    }

    #[test]
    fn only_the_bare_spelling_is_the_primary_key() {
        assert!(is_primary_key_attribute(&attribute("@id")));
        for raw in [
            "@identity",
            "@idx",
            "@id_foo",
            "@id()",
            "@id(x)",
            "@@id([a])",
        ] {
            assert!(!is_primary_key_attribute(&attribute(raw)), "{raw}");
        }
    }

    #[test]
    fn relation_matches_by_name() {
        assert!(is_relation_attribute(&attribute("@relation")));
        assert!(is_relation_attribute(&attribute(
            "@relation(fields: [a], references: [id])"
        )));
        assert!(!is_relation_attribute(&attribute("@relationship")));
        assert!(!is_relation_attribute(&attribute("@@relation(x)")));
    }

    #[test]
    fn field_attribute_name_strips_sigil_and_arguments() {
        assert_eq!(field_attribute_name("@length(min: 1)"), Some("length"));
        assert_eq!(field_attribute_name("@id"), Some("id"));
        assert_eq!(field_attribute_name("@@id([a])"), None);
        assert_eq!(field_attribute_name("id"), None);
    }

    // The two name readers of this module disagree by design: the loose
    // one (policy re-check, GHSA-69g4-xvcm-vm2j) reads every spelling
    // below as `id`/`relation`, the matchers here do not. The parser
    // refuses each of these on a field (the case variants, `@Id` and `@ID`,
    // as names the closed field lists do not contain, ADR 0019 D5), so
    // none is ever a key.
    #[test]
    fn the_loose_reader_takes_more_spellings_than_the_matchers() {
        use super::super::loose_attribute_names;
        assert_eq!(loose_attribute_names("@id"), ["id"]);
        assert!(is_primary_key_attribute(&attribute("@id")));
        for raw in ["@Id", "@ID", "@id;", "@id()", "@ id", "@i-d"] {
            assert_eq!(loose_attribute_names(raw), ["id"], "{raw}");
            assert!(!is_primary_key_attribute(&attribute(raw)), "{raw}");
        }
        for raw in ["@Relation(fields: [a])", "@rela-tion", "@ relation"] {
            assert_eq!(loose_attribute_names(raw), ["relation"], "{raw}");
            assert!(!is_relation_attribute(&attribute(raw)), "{raw}");
        }
    }
}
