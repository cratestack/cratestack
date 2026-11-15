//! `cratestack contract ops` against the runtime (cratestack#1388, review
//! M1). Its `requires_principal` is meant to drive a security allowlist, so
//! the one claim that must never be wrong is the safe one: **when the
//! analysis says an op requires a principal, an anonymous caller gets
//! nothing**, whatever the rule is made of (`none`/`every` over `auth()`,
//! a `detail` rule next to a `list` one, `hasRole`, `true`, `auth() == null`).
//!
//! For each model and each of `list`, `get` and `subscribe`, an anonymous
//! caller is run against real Postgres and the outcome is compared with the
//! verdict of `cratestack_policy`'s analysis applied to the rules the verb's
//! slot holds (`model_verb_policy_slots`, the table the macros generate from).
//! `subscribe` is judged by what the open feed delivers, not by whether it
//! opened: a relation policy opens (it is undecidable until a row exists)
//! and then decides each event.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok`.

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::axum::response::Response;
use cratestack::futures::StreamExt;
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::ModelVerb;
use cratestack_policy::{analyse_op_policy, model_rule_expression, model_verb_policy_slots};
use tokio::time::timeout;
use tower::util::ServiceExt;

const SCHEMA_PATH: &str = "tests/fixtures/contract_ops_agreement.cstack";
include_server_schema!(
    "tests/fixtures/contract_ops_agreement.cstack",
    db = Postgres
);

mod support;

use support::pg;

#[derive(Clone)]
struct Anonymous;

impl AuthProvider for Anonymous {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::anonymous()))
    }
}

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// What `cratestack contract ops` says about `model`'s `verb`: the policy
/// crate's analysis over the rules the verb's slot holds.
fn requires_principal(model: &cratestack_core::Model, verb: ModelVerb) -> bool {
    let actions: Vec<&str> = model_verb_policy_slots(verb)
        .iter()
        .flat_map(|slot| slot.actions().iter().copied())
        .collect();
    let rules = |directive: &str| -> Vec<&str> {
        model
            .attributes
            .iter()
            .filter_map(|a| model_rule_expression(&a.raw, directive, &actions))
            .map(|rule| rule.expect("fixture rules are well formed"))
            .collect()
    };
    let verdict = analyse_op_policy(&rules("@@allow"), &rules("@@deny"));
    assert!(verdict.unanalysed.is_empty(), "{}: {verdict:?}", model.name);
    verdict.requires_principal
}

/// `(listed any row, got any row by id)` for an anonymous caller.
macro_rules! read_anonymously {
    ($db:expr, $accessor:ident) => {{
        let anonymous = CratestackContext::anonymous();
        let listed = !$db
            .$accessor()
            .find_many()
            .run(&anonymous)
            .await
            .expect("list runs")
            .is_empty();
        let mut got = false;
        for id in [1_i64, 2] {
            got |= $db
                .$accessor()
                .find_unique(id)
                .run(&anonymous)
                .await
                .expect("get runs")
                .is_some();
        }
        (listed, got)
    }};
}

/// Create row 101 as a caller who passes every rule's create-time read.
macro_rules! create_as_member {
    ($db:expr, $accessor:ident, $input:ident) => {{
        let member = CratestackContext::authenticated([
            ("id".to_owned(), Value::Int(7)),
            (
                "roles".to_owned(),
                Value::List(vec![Value::String("a".into())]),
            ),
        ]);
        $db.$accessor()
            .bind(member)
            .create(cratestack_schema::$input {
                id: 101,
                published: true,
                ownerId: 7,
            })
            .run()
            .await
            .expect("row created");
    }};
}

async fn subscribe(router: &cratestack::axum::Router, model: &str) -> Response {
    let request = Request::get(format!("/rpc/subscribe/model.{model}.subscribe"))
        .header("accept", "text/event-stream, application/cbor")
        .body(Body::empty())
        .unwrap();
    router
        .clone()
        .oneshot(request)
        .await
        .expect("subscribe dispatch")
}

/// How many events the feed sends within `window` (none, if it was refused).
async fn delivered(response: Response, window: Duration) -> usize {
    if response.status() != StatusCode::OK {
        return 0;
    }
    let mut body = response.into_body().into_data_stream();
    let mut text = Vec::new();
    while let Ok(Some(chunk)) = timeout(window, body.next()).await {
        text.extend_from_slice(&chunk.expect("body chunk"));
    }
    String::from_utf8(text)
        .expect("SSE is UTF-8")
        .split("\n\n")
        .filter(|event| event.starts_with("event: message"))
        .count()
}

const PARENT_TABLES: [&str; 10] = [
    "ca_open", "ca_null", "ca_role", "ca_split", "ca_owner", "ca_and", "ca_or", "ca_none",
    "ca_every", "ca_some",
];

#[tokio::test]
async fn an_op_the_analysis_says_needs_a_principal_gives_an_anonymous_caller_nothing() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let pool = &test_pg.pool;
    let mut statements = vec!["DROP TABLE IF EXISTS cratestack_event_outbox".to_owned()];
    for table in PARENT_TABLES {
        statements.push(format!("DROP TABLE IF EXISTS {table}_item"));
        statements.push(format!("DROP TABLE IF EXISTS {table}"));
        statements.push(format!(
            "CREATE TABLE {table} (id BIGINT PRIMARY KEY, published BOOLEAN NOT NULL, owner_id BIGINT NOT NULL)"
        ));
        statements.push(format!(
            "INSERT INTO {table} VALUES (1, TRUE, 1), (2, TRUE, 2)"
        ));
    }
    for table in ["ca_none", "ca_every", "ca_some"] {
        statements.push(format!(
            "CREATE TABLE {table}_item (id BIGINT PRIMARY KEY, parent_id BIGINT NOT NULL, user_id BIGINT NOT NULL)"
        ));
        // Row 1 has no related rows (`every` is vacuously true there); row 2
        // has one, somebody else's.
        statements.push(format!("INSERT INTO {table}_item VALUES (1, 2, 7)"));
    }
    for statement in &statements {
        cratestack::sqlx::query(cratestack::sqlx::AssertSqlSafe(statement.clone()))
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
    }

    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        NoProcedures,
        (),
        CborCodec,
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let models = [
        "CaOpen", "CaNull", "CaRole", "CaSplit", "CaOwner", "CaAnd", "CaOr", "CaNone", "CaEvery",
        "CaSome",
    ];
    let mut feeds = Vec::new();
    for model in models {
        feeds.push((model, subscribe(&router, model).await));
    }
    create_as_member!(db, ca_open, CreateCaOpenInput);
    create_as_member!(db, ca_null, CreateCaNullInput);
    create_as_member!(db, ca_role, CreateCaRoleInput);
    create_as_member!(db, ca_split, CreateCaSplitInput);
    create_as_member!(db, ca_owner, CreateCaOwnerInput);
    create_as_member!(db, ca_and, CreateCaAndInput);
    create_as_member!(db, ca_or, CreateCaOrInput);
    create_as_member!(db, ca_none, CreateCaNoneInput);
    create_as_member!(db, ca_every, CreateCaEveryInput);
    create_as_member!(db, ca_some, CreateCaSomeInput);

    let window = Duration::from_millis(800);
    let counts = cratestack::futures::future::join_all(
        feeds
            .into_iter()
            .map(|(model, response)| async move { (model, delivered(response, window).await) }),
    )
    .await;
    let events: BTreeMap<&str, usize> = counts.into_iter().collect();

    let reads: BTreeMap<&str, (bool, bool)> = BTreeMap::from([
        ("CaOpen", read_anonymously!(db, ca_open)),
        ("CaNull", read_anonymously!(db, ca_null)),
        ("CaRole", read_anonymously!(db, ca_role)),
        ("CaSplit", read_anonymously!(db, ca_split)),
        ("CaOwner", read_anonymously!(db, ca_owner)),
        ("CaAnd", read_anonymously!(db, ca_and)),
        ("CaOr", read_anonymously!(db, ca_or)),
        ("CaNone", read_anonymously!(db, ca_none)),
        ("CaEvery", read_anonymously!(db, ca_every)),
        ("CaSome", read_anonymously!(db, ca_some)),
    ]);

    let schema = cratestack_parser::parse_schema(
        &std::fs::read_to_string(SCHEMA_PATH).expect("fixture is readable"),
    )
    .expect("fixture parses");
    let mut checked = 0;
    for name in models {
        let model = schema.models.iter().find(|m| m.name == name).unwrap();
        let (listed, got) = reads[name];
        let subscribed = events[name] > 0;
        for (verb, saw) in [
            (ModelVerb::List, listed),
            (ModelVerb::Get, got),
            (ModelVerb::Subscribe, subscribed),
        ] {
            let requires = requires_principal(model, verb);
            assert!(
                !(requires && saw),
                "{name} {verb:?}: the analysis says a principal is required, \
                 but an anonymous caller got rows"
            );
            // The converse, which a verdict of `false` need not satisfy in
            // general (it may over-approximate). It holds for these rules,
            // and keeps the test from passing vacuously on all-`true`.
            assert_eq!(requires, !saw, "{name} {verb:?}");
            checked += 1;
        }
    }
    assert_eq!(checked, 30);
}
