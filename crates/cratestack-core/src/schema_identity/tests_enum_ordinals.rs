//! Explicit enum ordinals in the schema digest: a string enum hashes exactly
//! as it did before the fields existed, and numbering, renumbering or
//! reserving each identify a different schema.

use super::tests::{empty, enum_decl};
use super::{canonical_bytes, schema_digest};
use crate::schema::{ReservedOrdinals, Schema, SourceSpan};

fn status(ordinals: Option<&[i64]>, reserved: Option<&[i64]>) -> Schema {
    let mut schema = empty();
    let mut e = enum_decl("Status", &["Pending", "Approved"]);
    if let Some(ordinals) = ordinals {
        for (variant, ordinal) in e.variants.iter_mut().zip(ordinals) {
            variant.ordinal = Some(*ordinal);
        }
    }
    e.reserved = reserved.map(|values| ReservedOrdinals {
        values: values.to_vec(),
        span: SourceSpan {
            start: 0,
            end: 0,
            line: 1,
        },
    });
    schema.enums.push(e);
    schema
}

#[test]
fn a_string_enum_canonicalises_without_ordinal_keys() {
    let bytes = String::from_utf8(canonical_bytes(&status(None, None))).unwrap();
    assert!(
        bytes.contains(r#""enums":[{"name":"Status","variants":["Pending","Approved"]}]"#),
        "{bytes}"
    );
}

#[test]
fn numbering_renumbering_and_reserving_each_move_the_digest() {
    let digests = [
        schema_digest(&status(None, None)),
        schema_digest(&status(Some(&[1, 2]), None)),
        schema_digest(&status(Some(&[1, 3]), None)),
        schema_digest(&status(Some(&[1, 3]), Some(&[2]))),
    ];
    for (i, a) in digests.iter().enumerate() {
        for b in &digests[i + 1..] {
            assert_ne!(a, b);
        }
    }
}
