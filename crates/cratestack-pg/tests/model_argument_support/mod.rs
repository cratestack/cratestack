//! The request bodies shared by `model_argument_validators_rest.rs` and
//! `model_argument_validators_rpc.rs`. One table drives both transports, so a
//! case cannot be asserted on one and forgotten on the other (CLAUDE.md,
//! "Transport parity").
//!
//! None of these needs a database: validation answers before any query, and
//! the pool behind the router cannot connect, so a request that reached the
//! database would fail with a different status than the 422 asserted here.

#![allow(dead_code)]

use std::time::Duration;

use cratestack::sqlx::PgPool;
use cratestack::sqlx::postgres::PgPoolOptions;
use serde_json::{Value, json};

/// A call to one procedure and what it must answer: `rejected` is the exact
/// public message of the 422 it must produce, `None` when validation must let
/// it through to the (empty) procedure body.
pub struct Case {
    pub label: &'static str,
    pub procedure: &'static str,
    pub body: Value,
    pub rejected: Option<&'static str>,
}

/// A pool whose database does not exist: port 1 refuses at once, so a request
/// that gets past validation to a query fails fast, and differently.
pub fn dead_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(500))
        .connect_lazy("postgres://nobody:none@127.0.0.1:1/none")
        .expect("a lazy pool never connects up front")
}

fn user(patch: Value) -> Value {
    let mut user = json!({"id": 1, "name": "Alice", "slug": "abc"});
    for (key, value) in patch.as_object().expect("an object patch") {
        user[key] = value.clone();
    }
    user
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
            "a valid model argument",
            "takeUser",
            json!({"args": user(json!({}))}),
            None,
        ),
        case(
            "an optional field present and valid",
            "takeUser",
            json!({"args": user(json!({"nick": "bob"}))}),
            None,
        ),
        // `secret` is `#[serde(skip)]`: the client's value is dropped and the
        // default (`""`, which fails `min: 8`) is never judged.
        case(
            "a @server_only field is never validated",
            "takeUser",
            json!({"args": user(json!({"secret": "x"}))}),
            None,
        ),
        case(
            "a valid model inside a type, in a list and optional",
            "takeMany",
            json!({"id": "x", "owners": [user(json!({}))], "extra": null}),
            None,
        ),
        // Rejected, each named by its path in the request body.
        case(
            "length on a model argument",
            "takeUser",
            json!({"args": user(json!({"name": "x"}))}),
            Some("field 'args.name' length 1 is below minimum 3"),
        ),
        case(
            "length on an optional field that is present",
            "takeUser",
            json!({"args": user(json!({"nick": "toolongnick"}))}),
            Some("field 'args.nick' length 11 exceeds maximum 6"),
        ),
        // The message carries the path and the bound, never the value sent.
        case(
            "a @readonly field the client sent",
            "takeUser",
            json!({"args": user(json!({"slug": "secret-looking-value"}))}),
            Some("field 'args.slug' length 20 exceeds maximum 5"),
        ),
        case(
            "a model one type down",
            "takeWrap",
            json!({"args": {"owner": user(json!({"name": "x"}))}}),
            Some("field 'args.owner.name' length 1 is below minimum 3"),
        ),
        case(
            "an element of a list of models, by its own argument name",
            "takeMany",
            json!({"id": "x", "owners": [user(json!({})), user(json!({"name": "x"}))], "extra": null}),
            Some("field 'owners[1].name' length 1 is below minimum 3"),
        ),
        case(
            "an optional model that is present",
            "takeMany",
            json!({"id": "x", "owners": [], "extra": user(json!({"name": "x"}))}),
            Some("field 'extra.name' length 1 is below minimum 3"),
        ),
        // The pool cannot connect: only validating before `run_isolated`
        // answers this 422; inside it, the first thing would be a connection.
        case(
            "an @isolation procedure refuses before it takes a connection",
            "takeIsolated",
            json!({"args": user(json!({"name": "x"}))}),
            Some("field 'args.name' length 1 is below minimum 3"),
        ),
    ]
}
