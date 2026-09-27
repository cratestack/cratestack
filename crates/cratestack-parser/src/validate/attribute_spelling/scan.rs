//! Finding where one attribute's raw text runs into the next.
//!
//! The field tokenizer (`crate::parse::attribute_spacing`) splits
//! attributes only at whitespace, so `@server_only@unique` reaches the
//! validators as one attribute. A block-level attribute is the whole
//! line (`crate::parse::models`, `crate::parse::views`), so
//! `@@audit @@paged` is one attribute too. Either way the second `@`
//! starts an attribute that nothing reads.
//!
//! An `@` counts only outside a string literal and outside any `(...)` /
//! `[...]` group, so `@default("a@b.c")`, `@@allow("read", auth().email ==
//! 'a@b')` and a `@@sql("""…""")` body using `@>` are left alone. The walk
//! itself is `cratestack_core::schema::attribute_text`, shared with the
//! parser's comment stripping and the macros' policy re-check, so all
//! three agree on what is inside a string. A trailing `//` comment never
//! reaches this module: the parser removes it when it builds the raw text.

use cratestack_core::schema::attribute_text::attribute_starts;

/// Byte offsets of every `@` in `raw`, after its own leading `@`s, that
/// starts another attribute.
pub(in crate::validate) fn run_on_offsets(raw: &str) -> Vec<usize> {
    attribute_starts(raw)
}

/// `raw` with the run-on attributes separated by `separator`.
pub(in crate::validate) fn separated(raw: &str, offsets: &[usize], separator: &str) -> String {
    let mut pieces = Vec::with_capacity(offsets.len() + 1);
    let mut start = 0;
    for &offset in offsets {
        pieces.push(raw[start..offset].trim());
        start = offset;
    }
    pieces.push(raw[start..].trim());
    pieces.join(separator)
}

#[cfg(test)]
mod tests {
    use super::run_on_offsets;

    #[test]
    fn finds_an_attribute_run_into_the_next() {
        assert_eq!(run_on_offsets("@server_only@unique"), [12]);
        assert_eq!(run_on_offsets("@@audit@@paged"), [7]);
        assert_eq!(run_on_offsets("@@audit @@paged"), [8]);
        assert_eq!(run_on_offsets("@default(1)@unique"), [11]);
        assert_eq!(run_on_offsets("@deny(x)@allow(y)"), [8]);
    }

    #[test]
    fn ignores_an_at_sign_inside_a_string_or_a_group() {
        for raw in [
            "@server_only",
            "@@paged",
            "@default(\"a@b.c\")",
            "@@allow(\"read\", auth().email == \"a@b\")",
            "@@allow('read', auth().email == 'ops@b.io')",
            "@@sql(\"SELECT \\\"a@b\\\" FROM t\")",
            "@@sql(\"\"\"\n  SELECT tags FROM t WHERE tags @> '{x}'\n\"\"\")",
            "@default(\"it's @ home\")",
            // A `)` inside a string must not close the group early.
            "@default(\":) mail@home\")",
            "@@allow('read', auth().note == ':) @x')",
            "@@sql(\"SELECT \\\":) @x\\\" FROM t\")",
        ] {
            assert_eq!(run_on_offsets(raw), Vec::<usize>::new(), "{raw}");
        }
    }
}
