//! [`FieldPath`]: where a value sits in the request body, built as the
//! validators descend and written only when one of them fails.
//!
//! A generated `ValidateFields` impl names each field it checks. Formatting
//! `args.owner.tags[1].label` into a `String` for every nested value would
//! cost an allocation per field on the path every valid request takes, to
//! produce text that only a rejection reads. A path is instead a chain of
//! borrowed segments on the call stack, one per level of descent, that
//! implements [`Display`] and is rendered by the validator that fails.

use core::fmt::{self, Display, Formatter};

/// A position in a request body: the root, a field of the value above it,
/// or an element of the list above it.
///
/// Displays as the dotted path the client wrote: `""` for the root,
/// `args.owner.tags[1].label` for a field of an element of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPath<'a> {
    /// The request body itself, which a procedure's arguments are fields of.
    Root,
    /// The field `.1` of the value at `.0`.
    Field(&'a FieldPath<'a>, &'a str),
    /// The element `.1` of the list at `.0`.
    Index(&'a FieldPath<'a>, usize),
}

impl<'a> FieldPath<'a> {
    /// The field `name` of the value at this path.
    pub fn field(&'a self, name: &'a str) -> FieldPath<'a> {
        FieldPath::Field(self, name)
    }

    /// The element `index` of the list at this path.
    pub fn index(&'a self, index: usize) -> FieldPath<'a> {
        FieldPath::Index(self, index)
    }
}

impl Display for FieldPath<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FieldPath::Root => Ok(()),
            // A field of the root is the whole path: no leading `.`.
            FieldPath::Field(FieldPath::Root, name) => f.write_str(name),
            FieldPath::Field(parent, name) => write!(f, "{parent}.{name}"),
            FieldPath::Index(parent, index) => write!(f, "{parent}[{index}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FieldPath;

    #[test]
    fn a_path_reads_as_the_client_wrote_it() {
        let root = FieldPath::Root;
        assert_eq!(root.to_string(), "");

        let args = root.field("args");
        assert_eq!(args.to_string(), "args");

        let owner = args.field("owner");
        let tags = owner.field("tags");
        let second = tags.index(1);
        let label = second.field("label");
        assert_eq!(label.to_string(), "args.owner.tags[1].label");
    }

    #[test]
    fn a_list_argument_is_indexed_from_the_root() {
        let root = FieldPath::Root;
        let tags = root.field("tags");
        let first = tags.index(0);
        assert_eq!(first.field("label").to_string(), "tags[0].label");
    }
}
