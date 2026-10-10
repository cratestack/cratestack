#![cfg(test)]
//! ADR 0019 D5, the other direction: every name a kind lists is accepted on
//! it in the shape its list gives, a listed name in another shape is refused,
//! and the lists agree with the placement checks that refuse the same names.
//! The refusals of unlisted names are in `tests_field_attribute_lists`.

use crate::tests_field_attribute_lists::{UNION, declaration, refusal};
use crate::{FieldHost, field_attribute_names, parse_schema};

/// Every name a kind lists parses on a field of that kind, in the shape its
/// list gives it. (`@relation` needs two models and is below.)
#[test]
fn every_listed_name_is_accepted_on_its_kind() {
    for host in FieldHost::ALL {
        for name in field_attribute_names(host) {
            let (ty, attribute) = match name {
                "@id" => continue, // is the declaration's own key; see below
                "@relation" => continue,
                "@default" => ("String", "@default(\"x\")"),
                // Only a validator with a SQL form gives it something to enforce.
                "@db_enforce" => ("String", "@length(min: 1) @db_enforce"),
                "@version" => ("Int", "@version"),
                "@length" => ("String", "@length(min: 1, max: 5)"),
                "@range" => ("Int", "@range(min: 0, max: 5)"),
                "@regex" => ("String", "@regex(\"^a$\")"),
                "@rename" => ("String", "@rename(from = \"old\")"),
                "@from" => ("Int", "@from(A.id)"),
                other => ("String", other),
            };
            let source = declaration(host, ty, attribute);
            parse_schema(&source).unwrap_or_else(|e| panic!("{host:?} {name}: {e}\n{source}"));
        }
    }
    parse_schema("model T {\n  k Int @id\n}\n").expect("@id on a model");
    parse_schema(
        "model A {\n  id Int @id\n}\nview V from A {\n  k Int @id\n  @@sql(\"SELECT 1\")\n}\n",
    )
    .expect("@id on a view");
    let relation = "model A {\n  id Int @id\n}\nmodel B {\n  id Int @id\n  aId Int\n  a A \
                    @relation(fields: [aId], references: [id])\n}\n";
    parse_schema(relation).expect("@relation on a model");
    parse_schema(
        "model A {\n  id Int @id\n}\nmixin M {\n  aId Int\n  a A \
         @relation(fields: [aId], references: [id])\n}\nmodel B {\n  id Int @id\n  @use(M)\n}\n",
    )
    .expect("@relation on a mixin");
}

#[test]
fn a_listed_name_in_the_wrong_shape_is_refused() {
    for (attribute, needle) in [
        ("@default", "`@default` takes an argument list"),
        ("@default()", "`@default()` has an empty argument list"),
        ("@relation", "`@relation` takes an argument list"),
        ("@unique(x)", "`@unique` does not take arguments"),
        ("@pii()", "`@pii` does not take arguments"),
        ("@computed()", "unsupported computed field directive"),
    ] {
        let message = refusal(FieldHost::Model, "String", attribute);
        assert!(message.contains(needle), "{attribute}: {message}");
    }
}

/// The placement checks that give their own message and the lists must say
/// the same thing, since both can refuse the same name.
#[test]
fn the_lists_agree_with_the_placement_checks() {
    use std::collections::BTreeSet;
    let names = |host| {
        field_attribute_names(host)
            .into_iter()
            .collect::<BTreeSet<_>>()
    };
    let (model, mixin) = (names(FieldHost::Model), names(FieldHost::Mixin));
    // A mixin is a model's fields minus the two names refused on it.
    let expected: BTreeSet<_> = model
        .difference(&BTreeSet::from(["@id", "@computed"]))
        .copied()
        .collect();
    assert_eq!(mixin, expected);
    for host in [FieldHost::Type, FieldHost::Auth] {
        assert!(!names(host).contains("@server_only"), "{host:?}");
    }
    let computed: Vec<_> = FieldHost::ALL
        .into_iter()
        .filter(|host| names(*host).contains("@computed"))
        .collect();
    assert_eq!(computed, [FieldHost::Model, FieldHost::Type]);
    let rename: Vec<_> = FieldHost::ALL
        .into_iter()
        .filter(|host| names(*host).contains("@rename"))
        .collect();
    assert_eq!(rename, [FieldHost::Model, FieldHost::Mixin]);
    // The union is the 19 names the readers use; a new one is a decision.
    let union: BTreeSet<_> = FieldHost::ALL.into_iter().flat_map(names).collect();
    assert_eq!(union, BTreeSet::from(UNION));
    assert!(names(FieldHost::Auth).is_empty());
}

/// `removed_attributes` runs first, so these keep their own guidance.
#[test]
fn a_removed_name_keeps_its_own_message() {
    for host in FieldHost::ALL {
        let message = refusal(host, "String", "@pb(1)");
        assert!(message.contains("removed in 0.8.5"), "{host:?}: {message}");
        assert!(
            !message.contains("unsupported attribute"),
            "{host:?}: {message}"
        );
    }
}
