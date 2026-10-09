//! The request bodies shared by `type_validators_rest.rs` and
//! `type_validators_rpc.rs`. One table drives both transports, so a case
//! cannot be asserted on one and forgotten on the other (CLAUDE.md,
//! "Transport parity").

use serde_json::{Value, json};

/// A call to one procedure and what it must answer: `rejected` is the exact
/// public message of the 422 it must produce, `None` when it must succeed.
pub struct Case {
    pub label: &'static str,
    pub procedure: &'static str,
    pub body: Value,
    pub rejected: Option<&'static str>,
}

fn account(patch: Value) -> Value {
    let mut account = json!({
        "owner": {
            "name": "Alice",
            "email": "alice@example.com",
            "tags": [{ "label": "ab" }, { "label": "abcdefgh" }],
            "backup": null
        },
        "age": 30,
        "code": "ABC-12",
        "currency": "XAF",
        "site": "https://example.com",
        "digest": [1, 2, 3, 4]
    });
    merge(&mut account, patch);
    json!({ "args": account })
}

fn merge(into: &mut Value, patch: Value) {
    if let (Some(into), Value::Object(patch)) = (into.as_object_mut(), patch) {
        for (key, value) in patch {
            match into.get_mut(&key) {
                Some(slot) if slot.is_object() && value.is_object() => merge(slot, value),
                _ => {
                    into.insert(key, value);
                }
            }
        }
    }
}

pub fn cases() -> Vec<Case> {
    let case = |label, procedure, body, rejected| Case {
        label,
        procedure,
        body,
        rejected,
    };
    vec![
        // Accepted.
        case(
            "greeting at the minimum",
            "greet",
            json!({"args": {"message": "abc"}}),
            None,
        ),
        case(
            "a fully valid account",
            "openAccount",
            account(json!({})),
            None,
        ),
        case(
            "optional fields omitted",
            "openAccount",
            account(json!({"currency": null, "site": null})),
            None,
        ),
        case(
            "list and optional arguments valid",
            "relabel",
            json!({"id": "x", "tags": [{"label": "ab"}], "extra": null}),
            None,
        ),
        case(
            "a type with no validator takes anything",
            "plain",
            json!({"args": {"note": ""}}),
            None,
        ),
        // Rejected, each named by its path in the request body.
        case(
            "length on an argument type",
            "greet",
            json!({"args": {"message": "hi"}}),
            Some("field 'args.message' length 2 is below minimum 3"),
        ),
        case(
            "length one type down",
            "openAccount",
            account(json!({"owner": {"name": "Al"}})),
            Some("field 'args.owner.name' length 2 is below minimum 3"),
        ),
        case(
            "email one type down",
            "openAccount",
            account(json!({"owner": {"email": "not-an-email"}})),
            Some("field 'args.owner.email' is not a valid email address"),
        ),
        case(
            "an element of a list of types",
            "openAccount",
            account(json!({"owner": {"tags": [{"label": "ab"}, {"label": "abcdefghi"}]}})),
            Some("field 'args.owner.tags[1].label' length 9 exceeds maximum 8"),
        ),
        case(
            "an optional type that is present",
            "openAccount",
            account(json!({"owner": {"backup": {"label": "x"}}})),
            Some("field 'args.owner.backup.label' length 1 is below minimum 2"),
        ),
        case(
            "range below",
            "openAccount",
            account(json!({"age": 17})),
            Some("field 'args.age' is below minimum 18"),
        ),
        case(
            "range above",
            "openAccount",
            account(json!({"age": 121})),
            Some("field 'args.age' exceeds maximum 120"),
        ),
        case(
            "regex",
            "openAccount",
            account(json!({"code": "abc-12"})),
            Some("field 'args.code' does not match required pattern"),
        ),
        case(
            "iso4217 on an optional that is present",
            "openAccount",
            account(json!({"currency": "xaf"})),
            Some("field 'args.currency' must be a 3-letter uppercase ISO 4217 code"),
        ),
        case(
            "uri on an optional that is present",
            "openAccount",
            account(json!({"site": "not a uri"})),
            Some("field 'args.site' is not a valid URI"),
        ),
        case(
            "length on bytes",
            "openAccount",
            account(json!({"digest": [1, 2, 3]})),
            Some("field 'args.digest' length 3 is below minimum 4"),
        ),
        case(
            "a list argument, by its own name",
            "relabel",
            json!({"id": "x", "tags": [{"label": "ab"}, {"label": "x"}], "extra": null}),
            Some("field 'tags[1].label' length 1 is below minimum 2"),
        ),
        case(
            "an optional argument that is present",
            "relabel",
            json!({"id": "x", "tags": [], "extra": {"label": "x"}}),
            Some("field 'extra.label' length 1 is below minimum 2"),
        ),
    ]
}
