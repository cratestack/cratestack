//! Enum-level diff: emit CREATE TYPE / ALTER TYPE ADD VALUE / DROP TYPE
//! ops based on the prev/next enum sets.

use std::collections::BTreeMap;

use cratestack_core::Schema;

use crate::ir::{AlterEnumAddVariant, CreateEnum, DropEnum, Op};

/// Returns `(create_enums, alter_enums, drop_enums)`.
pub(super) fn diff_enums(prev: &Schema, next: &Schema) -> (Vec<Op>, Vec<Op>, Vec<Op>) {
    let prev_enums: BTreeMap<&str, Vec<&str>> = prev
        .enums
        .iter()
        .map(|decl| {
            (
                decl.name.as_str(),
                decl.variants.iter().map(|v| v.name.as_str()).collect(),
            )
        })
        .collect();
    let next_enums: BTreeMap<&str, Vec<&str>> = next
        .enums
        .iter()
        .map(|decl| {
            (
                decl.name.as_str(),
                decl.variants.iter().map(|v| v.name.as_str()).collect(),
            )
        })
        .collect();

    let mut create_enums = Vec::new();
    let mut alter_enums = Vec::new();
    let mut drop_enums = Vec::new();

    for name in prev_enums.keys() {
        if !next_enums.contains_key(name) {
            drop_enums.push(Op::DropEnum(DropEnum {
                name: (*name).to_owned(),
            }));
        }
    }
    for (name, variants) in &next_enums {
        match prev_enums.get(name) {
            None => create_enums.push(Op::CreateEnum(CreateEnum {
                name: (*name).to_owned(),
                variants: variants.iter().map(|v| (*v).to_owned()).collect(),
            })),
            Some(prev_variants) => {
                // Variants present in next but not prev → ADD VALUE.
                // Variant removal is out of scope for this slice
                // (requires the Postgres swap-dance, plus a backfill
                // plan for referencing rows).
                let prev_set: std::collections::HashSet<&str> =
                    prev_variants.iter().copied().collect();
                for variant in variants {
                    if !prev_set.contains(variant) {
                        alter_enums.push(Op::AlterEnumAddVariant(AlterEnumAddVariant {
                            name: (*name).to_owned(),
                            value: (*variant).to_owned(),
                        }));
                    }
                }
            }
        }
    }

    (create_enums, alter_enums, drop_enums)
}
