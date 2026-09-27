//! The argument of a rename marker, `@@rename(from = "<old_table>")` on a
//! model and `@rename(from = "<old_column>")` on a field.
//!
//! One reader for both sides of GHSA-69g4-xvcm-vm2j: `cratestack-migrate`
//! (`convert/renames.rs`) takes the old name from it, and
//! `cratestack-parser` refuses a `@@rename` or `@rename` it does not read.
//! Before, the migrator read `from = "…"` only and treated anything else
//! as no marker at all, so `@@rename(from: "old")` passed `cratestack
//! check` and the next migration dropped the old table and created the new
//! one — its rows gone — instead of renaming it; `title String
//! @rename(from: "name")` did the same to a column.

/// The one argument shape a rename marker takes, for error messages.
pub const RENAME_ARGUMENT_FORM: &str = "from = \"<old_name>\"";

/// The old name in `arguments`, the text inside a rename marker's
/// parentheses: `from`, `=`, then one double-quoted, non-empty name with
/// no `"` or `\` in it, whitespace allowed around each part. `None` for
/// anything else.
pub fn parse_rename_from(arguments: &str) -> Option<&str> {
    let rest = arguments.trim().strip_prefix("from")?.trim_start();
    let value = rest.strip_prefix('=')?.trim_start();
    let name = value.strip_prefix('"')?.strip_suffix('"')?;
    let plain = !name.is_empty() && !name.contains(['"', '\\']);
    plain.then_some(name)
}

/// The old name in a whole rename marker `raw`, as `cratestack-migrate`
/// reads it: `marker` (`"@rename"` or `"@@rename"`), directly its `(`,
/// arguments [`parse_rename_from`] takes, and a `)` that ends the text.
/// `None` for anything else — which the migrator treats as no marker.
pub fn rename_marker_from<'a>(raw: &'a str, marker: &str) -> Option<&'a str> {
    let arguments = raw
        .strip_prefix(marker)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    parse_rename_from(arguments)
}

#[cfg(test)]
mod tests {
    use super::{parse_rename_from, rename_marker_from};

    #[test]
    fn a_whole_marker_is_read_only_when_the_line_ends_at_its_parenthesis() {
        assert_eq!(
            rename_marker_from("@rename(from = \"name\")", "@rename"),
            Some("name")
        );
        assert_eq!(
            rename_marker_from("@@rename(from=\"docs\")", "@@rename"),
            Some("docs")
        );
        for raw in [
            "@rename",
            "@rename()",
            "@rename(from = \"name\"),",
            "@rename(from = \"name\"))",
            "@rename (from = \"name\")",
            "@renamed(from = \"name\")",
            "@@rename(from = \"name\")",
        ] {
            assert_eq!(rename_marker_from(raw, "@rename"), None, "{raw}");
        }
    }

    #[test]
    fn the_accepted_form_gives_the_old_name() {
        for arguments in [
            "from = \"old_docs\"",
            "from=\"old_docs\"",
            "  from  =  \"old_docs\"  ",
        ] {
            assert_eq!(
                parse_rename_from(arguments),
                Some("old_docs"),
                "{arguments}"
            );
        }
    }

    #[test]
    fn every_other_form_gives_nothing() {
        for arguments in [
            "from: \"old\"",
            "\"old\"",
            "old",
            "from = old",
            "from = 'old'",
            "from = \"\"",
            "from = \"a\" \"b\"",
            "from = \"a\\\"b\"",
            "fromage = \"old\"",
            "to = \"old\"",
            "from = \"old\", to = \"new\"",
            "from == \"old\"",
            "",
        ] {
            assert_eq!(parse_rename_from(arguments), None, "{arguments}");
        }
    }
}
