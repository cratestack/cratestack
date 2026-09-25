//! The cases, the seed and the comparison for `tests/server_only_query.rs`.
//! In a directory so cargo does not build it as a test binary of its own.
//!
//! A case is a request with a `{f}` placeholder where a field name goes.
//! It runs three times: with the `@server_only` field's public twin, which
//! must answer `200` (so the operator is one the query contract really
//! offers for that type); with the `@server_only` field; and with
//! [`UNDECLARED`], a name no model declares. The last two must answer
//! byte for byte the same, once the name is swapped back. A refusal that
//! differed would itself tell a caller that the field exists.

use serde_json::{Value as Json, json};

/// A field name no fixture model declares.
pub const UNDECLARED: &str = "undeclared";

/// The `@server_only` values. None may appear in any response.
pub const SECRETS: [&str; 5] = [
    "HUNTER2",
    "SWORDFISH",
    "RECOVER-1",
    "PET-TOKEN-A",
    "PET-TOKEN-B",
];

pub const SEED: [&str; 5] = [
    "DROP TABLE IF EXISTS so_qry_pets, so_qry_owners",
    "CREATE TABLE so_qry_owners (id BIGINT PRIMARY KEY, name TEXT NOT NULL, age BIGINT NOT NULL, \
     nickname TEXT, secret TEXT NOT NULL, pin BIGINT NOT NULL, recovery TEXT)",
    "CREATE TABLE so_qry_pets (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
     label TEXT NOT NULL, token TEXT NOT NULL)",
    // Every probe below singles out one of the two owners (or pets), so a
    // `@server_only` key that still filtered would answer a strict subset.
    "INSERT INTO so_qry_owners (id, name, age, nickname, secret, pin, recovery) VALUES \
     (1, 'alice', 30, 'ally', 'HUNTER2', 4242, 'RECOVER-1'), \
     (2, 'bob', 40, NULL, 'SWORDFISH', 1111, NULL)",
    "INSERT INTO so_qry_pets (id, owner_id, label, token) VALUES \
     (10, 1, 'rex', 'PET-TOKEN-A'), (11, 2, 'tom', 'PET-TOKEN-B')",
];

/// One list query: `/<plural>?<query>`, `{f}` naming `server_only` or its
/// `twin`.
pub struct Case {
    pub plural: &'static str,
    pub query: &'static str,
    pub server_only: &'static str,
    pub twin: &'static str,
}

impl Case {
    pub fn with(&self, field: &str) -> String {
        self.query.replace("{f}", field)
    }
}

const fn case(
    plural: &'static str,
    query: &'static str,
    server_only: &'static str,
    twin: &'static str,
) -> Case {
    Case {
        plural,
        query,
        server_only,
        twin,
    }
}

const OWNERS: &str = "so_qry_owners";
const PETS: &str = "so_qry_pets";

/// Every filter operator the generated `build_<model>_filter_expr` offers
/// (`cratestack-macros/src/axum/filter_arms.rs`), for each type and arity
/// that offers it, bare and inside `where=`/`or=`, and through a to-one,
/// a to-many (`some`/`every`/`none`) and a two-hop relation path. There is
/// no `__endsWith` operator on any field.
pub fn filter_cases() -> Vec<Case> {
    vec![
        // `String`, required: equality, list, comparison, text.
        case(OWNERS, "{f}=HUNTER2", "secret", "name"),
        case(OWNERS, "{f}__eq=HUNTER2", "secret", "name"),
        case(OWNERS, "{f}__ne=SWORDFISH", "secret", "name"),
        case(OWNERS, "{f}__in=HUNTER2,nope", "secret", "name"),
        case(OWNERS, "{f}__lt=I", "secret", "name"),
        case(OWNERS, "{f}__lte=HUNTER2", "secret", "name"),
        case(OWNERS, "{f}__gt=I", "secret", "name"),
        case(OWNERS, "{f}__gte=SWORDFISH", "secret", "name"),
        case(OWNERS, "{f}__contains=UNT", "secret", "name"),
        case(OWNERS, "{f}__startsWith=HUN", "secret", "name"),
        // `Int`, required: equality, list, comparison.
        case(OWNERS, "{f}=4242", "pin", "age"),
        case(OWNERS, "{f}__ne=1111", "pin", "age"),
        case(OWNERS, "{f}__in=4242,7", "pin", "age"),
        case(OWNERS, "{f}__lt=2000", "pin", "age"),
        case(OWNERS, "{f}__lte=1111", "pin", "age"),
        case(OWNERS, "{f}__gt=2000", "pin", "age"),
        case(OWNERS, "{f}__gte=4242", "pin", "age"),
        // `String?`: equality, list, text, null checks.
        case(OWNERS, "{f}=RECOVER-1", "recovery", "nickname"),
        case(OWNERS, "{f}__ne=RECOVER-1", "recovery", "nickname"),
        case(OWNERS, "{f}__in=RECOVER-1", "recovery", "nickname"),
        case(OWNERS, "{f}__contains=COVER", "recovery", "nickname"),
        case(OWNERS, "{f}__startsWith=REC", "recovery", "nickname"),
        case(OWNERS, "{f}__isNull=true", "recovery", "nickname"),
        case(OWNERS, "{f}__isNull=false", "recovery", "nickname"),
        // The `where=` and `or=` grammars reach the same builder.
        case(OWNERS, "where={f}=HUNTER2", "secret", "name"),
        case(OWNERS, "where=not({f}__startsWith=HUN)", "secret", "name"),
        case(OWNERS, "where=name=nobody|{f}__gt=I", "secret", "name"),
        case(
            OWNERS,
            "or=name=nobody|{f}__startsWith=HUN",
            "secret",
            "name",
        ),
        // To-one relation, one and two hops.
        case(PETS, "owner.{f}=HUNTER2", "secret", "name"),
        case(PETS, "owner.{f}__startsWith=HUN", "secret", "name"),
        case(PETS, "owner.{f}__gt=2000", "pin", "age"),
        case(PETS, "owner.{f}__isNull=true", "recovery", "nickname"),
        case(PETS, "where=owner.{f}=HUNTER2", "secret", "name"),
        case(
            PETS,
            "owner.pets.some.{f}__startsWith=PET-TOKEN-A",
            "token",
            "label",
        ),
        // To-many relation, every quantifier.
        case(OWNERS, "pets.some.{f}=PET-TOKEN-A", "token", "label"),
        case(
            OWNERS,
            "pets.every.{f}__startsWith=PET-TOKEN-A",
            "token",
            "label",
        ),
        case(OWNERS, "pets.none.{f}__contains=TOKEN-A", "token", "label"),
    ]
}

/// `sort=` and its `orderBy=` alias, both directions, as a later key, and
/// through a to-one relation (the only kind a sort may cross).
pub fn sort_cases() -> Vec<Case> {
    vec![
        case(OWNERS, "sort={f}", "secret", "name"),
        case(OWNERS, "sort=-{f}", "secret", "name"),
        case(OWNERS, "orderBy={f}", "secret", "name"),
        case(OWNERS, "orderBy=-{f}", "secret", "name"),
        case(OWNERS, "sort=age,-{f}", "secret", "name"),
        case(OWNERS, "sort={f}", "pin", "age"),
        case(OWNERS, "sort=-{f}", "recovery", "nickname"),
        case(PETS, "sort={f}", "token", "label"),
        case(PETS, "sort=owner.{f}", "secret", "name"),
        case(PETS, "sort=-owner.{f}", "pin", "age"),
    ]
}

/// One `FindMany<SoQryOwner>` `where` entry: the `@server_only` field, its
/// operator object, then the twin and an operator object that narrows the
/// twin to one owner.
pub struct FindManyWhere {
    pub server_only: &'static str,
    pub probe: Json,
    pub twin: &'static str,
    pub twin_probe: Json,
}

/// `(server_only, operator, value, twin, twin value)`, values as JSON text.
/// Every `FieldFilterInput` operator, on each type and arity that offers it.
const FIND_MANY_WHERE: [(&str, &str, &str, &str, &str); 23] = [
    ("secret", "eq", r#""HUNTER2""#, "name", r#""alice""#),
    ("secret", "ne", r#""SWORDFISH""#, "name", r#""bob""#),
    ("secret", "in", r#"["HUNTER2"]"#, "name", r#"["alice"]"#),
    ("secret", "lt", r#""I""#, "name", r#""b""#),
    ("secret", "lte", r#""HUNTER2""#, "name", r#""alice""#),
    ("secret", "gt", r#""I""#, "name", r#""b""#),
    ("secret", "gte", r#""SWORDFISH""#, "name", r#""bob""#),
    ("secret", "contains", r#""UNT""#, "name", r#""lic""#),
    ("secret", "startsWith", r#""HUN""#, "name", r#""al""#),
    ("pin", "eq", "4242", "age", "30"),
    ("pin", "ne", "1111", "age", "40"),
    ("pin", "in", "[4242]", "age", "[30]"),
    ("pin", "lt", "2000", "age", "35"),
    ("pin", "lte", "1111", "age", "30"),
    ("pin", "gt", "2000", "age", "35"),
    ("pin", "gte", "4242", "age", "40"),
    ("recovery", "eq", r#""RECOVER-1""#, "nickname", r#""ally""#),
    ("recovery", "ne", r#""RECOVER-1""#, "nickname", r#""x""#),
    (
        "recovery",
        "in",
        r#"["RECOVER-1"]"#,
        "nickname",
        r#"["ally"]"#,
    ),
    ("recovery", "contains", r#""COVER""#, "nickname", r#""ll""#),
    ("recovery", "startsWith", r#""REC""#, "nickname", r#""al""#),
    ("recovery", "isNull", "true", "nickname", "true"),
    ("recovery", "isNull", "false", "nickname", "false"),
];

pub fn find_many_where_cases() -> Vec<FindManyWhere> {
    let operator = |name: &str, value: &str| {
        let value: Json = serde_json::from_str(value).expect("case value is JSON");
        json!({ name: value })
    };
    FIND_MANY_WHERE
        .iter()
        .map(
            |&(server_only, name, value, twin, twin_value)| FindManyWhere {
                server_only,
                probe: operator(name, value),
                twin,
                twin_probe: operator(name, twin_value),
            },
        )
        .collect()
}

/// `FindMany<SoQryOwner>` `orderBy` keys: each `@server_only` field and its
/// twin, both directions.
pub fn find_many_sort_cases() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut cases = Vec::new();
    for (server_only, twin) in [("secret", "name"), ("pin", "age"), ("recovery", "nickname")] {
        for direction in ["asc", "desc"] {
            cases.push((server_only, twin, direction));
        }
    }
    cases
}

/// A response: status code and body.
pub type Answer = (u16, Vec<u8>);

/// What one case found wrong, if anything; `None` when it held. Collected
/// rather than asserted, so one run reports every case that fails.
pub fn compare(
    what: &str,
    server_only: &str,
    twin: Option<&Answer>,
    probe: &Answer,
    undeclared: &Answer,
) -> Option<String> {
    for (label, (_, body)) in [("probe", probe), ("undeclared", undeclared)] {
        let text = String::from_utf8_lossy(body);
        if let Some(secret) = SECRETS.iter().find(|secret| text.contains(**secret)) {
            return Some(format!(
                "{what}: {label} response carries `{secret}`: {text}"
            ));
        }
    }
    if let Some((status, body)) = twin
        && *status != 200
    {
        let text = String::from_utf8_lossy(body);
        return Some(format!(
            "{what}: the public twin must be accepted, got {status}: {text}"
        ));
    }
    let expected = String::from_utf8_lossy(&undeclared.1).replace(UNDECLARED, server_only);
    let actual = String::from_utf8_lossy(&probe.1);
    if probe.0 != undeclared.0 || actual != expected {
        return Some(format!(
            "{what}: a @server_only key must be refused like an undeclared one; got {} {actual}, \
             an undeclared key got {} {expected}",
            probe.0, undeclared.0
        ));
    }
    None
}
