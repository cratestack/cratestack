//! A request can never filter or sort by a `@server_only` field, on REST or
//! RPC, whether the model has a `@computed` field or not.
//!
//! List routes used to take their filter and sort keys from every stored
//! scalar field, `@server_only` ones included. `?secret=HUNTER2` answered
//! the row and `?secret=WRONG` answered `[]`, so the value never left the
//! server but could be tested. `?secret__startsWith=` rebuilt it one
//! character at a time, and `?sort=secret` ordered by it. RPC re-enters
//! the same parser (`cratestack-axum/src/rpc/synthesize.rs`), and a
//! `FindMany<Model>` procedure argument decoded the same keys into its
//! generated `<Model>Where`/`<Model>SortField`.
//!
//! Every case in `server_only_query_support` must now be refused exactly
//! as a field the model does not declare. One database test runs them all
//! and reports every case that fails, not only the first. Postgres-backed:
//! run it with `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip
//! that still prints `ok` (CLAUDE.md, "Critical test gotcha").

mod server_only_query_support;
mod support;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::Request;
use cratestack::include_server_schema;
use cratestack::serde_json::{self, Value as Json, json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use server_only_query_support::{
    Answer, SEED, UNDECLARED, compare, filter_cases, find_many_sort_cases, find_many_where_cases,
    sort_cases,
};
use support::pg;
use tower::util::ServiceExt;

#[derive(Clone)]
struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

async fn send(router: cratestack::axum::Router, request: Request<Body>) -> Answer {
    let (mut parts, body) = request.into_parts();
    for header in ["accept", "content-type"] {
        parts.headers.insert(
            header,
            JsonCodec::CONTENT_TYPE.parse().expect("header value"),
        );
    }
    let response = router
        .oneshot(Request::from_parts(parts, body))
        .await
        .expect("request should run");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    (status, bytes.to_vec())
}

async fn post(router: cratestack::axum::Router, path: &str, body: &Json) -> Answer {
    let request = Request::post(path)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .expect("request should build");
    send(router, request).await
}

/// `FindMany<SoQryOwner>` implemented as an application would: straight
/// through the generated `build_so_qry_owner_query_from_find_many`, with
/// `id` as a tiebreak so two answers compare byte for byte.
macro_rules! find_many_procedures {
    () => {
        #[derive(Clone)]
        pub(crate) struct Procedures;

        impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
            async fn so_qry_find_owners(
                &self,
                db: &cratestack_schema::Cratestack,
                ctx: &CratestackContext,
                args: cratestack_schema::procedures::so_qry_find_owners::Args,
                _authorized: cratestack_schema::procedures::so_qry_find_owners::Authorized,
            ) -> Result<Vec<cratestack_schema::SoQryOwner>, cratestack::CratestackError> {
                cratestack_schema::build_so_qry_owner_query_from_find_many(db, &args.query)
                    .order_by(cratestack_schema::so_qry_owner::id().asc())
                    .run(ctx)
                    .await
            }
        }
    };
}

/// Runs every list case through `list` (a transport's way of listing
/// `/<plural>?<query>`), and every `FindMany` case through `find_many`
/// (its way of calling `soQryFindOwners`), and returns what failed.
async fn run_cases<L, LF, F, FF>(transport: &str, list: L, find_many: F) -> Vec<String>
where
    L: Fn(&'static str, String) -> LF,
    LF: core::future::Future<Output = Answer>,
    F: Fn(Json) -> FF,
    FF: core::future::Future<Output = Answer>,
{
    let mut failures = Vec::new();
    for (kind, cases) in [("filter", filter_cases()), ("sort", sort_cases())] {
        for case in &cases {
            let what = format!("{transport} {kind} /{}?{}", case.plural, case.query);
            let twin = list(case.plural, case.with(case.twin)).await;
            let probe = list(case.plural, case.with(case.server_only)).await;
            let undeclared = list(case.plural, case.with(UNDECLARED)).await;
            failures.extend(compare(
                &what,
                case.server_only,
                Some(&twin),
                &probe,
                &undeclared,
            ));
        }
    }

    let unfiltered = find_many(json!({ "query": {} })).await;
    for case in find_many_where_cases() {
        let what = format!(
            "{transport} FindMany where {}: {}",
            case.server_only, case.probe
        );
        let body = |field: &str, filter: &Json| json!({ "query": { "where": { field: filter } } });
        let twin = find_many(body(case.twin, &case.twin_probe)).await;
        if twin.0 != 200 || twin.1 == unfiltered.1 {
            failures.push(format!(
                "{what}: the twin `{}` {} must narrow the owners, got {} {}",
                case.twin,
                case.twin_probe,
                twin.0,
                String::from_utf8_lossy(&twin.1)
            ));
        }
        let probe = find_many(body(case.server_only, &case.probe)).await;
        let undeclared = find_many(body(UNDECLARED, &case.probe)).await;
        failures.extend(compare(&what, case.server_only, None, &probe, &undeclared));
    }
    for (server_only, twin, direction) in find_many_sort_cases() {
        let what = format!("{transport} FindMany orderBy {server_only} {direction}");
        let body = |field: &str| json!({ "query": { "orderBy": [{ "field": field, "direction": direction }] } });
        let twin = find_many(body(twin)).await;
        let probe = find_many(body(server_only)).await;
        let undeclared = find_many(body(UNDECLARED)).await;
        failures.extend(compare(
            &what,
            server_only,
            Some(&twin),
            &probe,
            &undeclared,
        ));
    }
    failures
}

mod rest {
    use super::*;

    include_server_schema!("tests/fixtures/server_only_query.cstack", db = Postgres);
    find_many_procedures!();

    pub(super) async fn cases(pool: &cratestack::sqlx::PgPool) -> Vec<String> {
        let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
        let models = cratestack_schema::axum::model_router(db.clone(), (), JsonCodec, AllowAllAuth);
        let procedures =
            cratestack_schema::axum::procedure_router(db, Procedures, (), JsonCodec, AllowAllAuth);
        let list = |plural: &'static str, query: String| {
            let router = models.clone();
            async move {
                // The query is sent as a client would type it; only what
                // a URL cannot carry raw is escaped.
                let query = query.replace('|', "%7C").replace(' ', "%20");
                let request = Request::get(format!("/{plural}?{query}"))
                    .body(Body::empty())
                    .expect("request should build");
                send(router, request).await
            }
        };
        let find_many = |body: Json| {
            let router = procedures.clone();
            async move { post(router, "/$procs/soQryFindOwners", &body).await }
        };
        run_cases("REST", list, find_many).await
    }
}

mod rpc {
    use super::*;
    use cratestack::rpc::{RpcListInput, RpcListPredicate};

    include_server_schema!("tests/fixtures/server_only_query_rpc.cstack", db = Postgres);
    find_many_procedures!();

    /// The REST query as the `RpcListInput` a client would send: `sort` and
    /// its `orderBy` alias in `sort`, `where=`/`or=` in their own slots,
    /// every other pair a predicate in `filters`.
    fn list_input(query: &str) -> RpcListInput {
        let mut input = RpcListInput::default();
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').expect("key=value");
            let value = value.to_owned();
            match key {
                "sort" | "orderBy" => input.sort = Some(value),
                "where" => input.where_expr = Some(value),
                "or" => input.or = Some(value),
                _ => input.filters.push(RpcListPredicate {
                    key: key.to_owned(),
                    value,
                }),
            }
        }
        input
    }

    fn op(plural: &str) -> &'static str {
        match plural {
            "so_qry_owners" => "model.SoQryOwner.list",
            "so_qry_pets" => "model.SoQryPet.list",
            other => panic!("no RPC list op for `{other}`"),
        }
    }

    pub(super) async fn cases(pool: &cratestack::sqlx::PgPool) -> Vec<String> {
        let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
        let router = cratestack_schema::axum::rpc_router(
            db,
            Procedures,
            (),
            JsonCodec,
            AllowAllAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        );
        let list = |plural: &'static str, query: String| {
            let router = router.clone();
            async move {
                let input = serde_json::to_value(list_input(&query)).expect("RpcListInput");
                post(router, &format!("/rpc/{}", op(plural)), &input).await
            }
        };
        let find_many = |body: Json| {
            let router = router.clone();
            async move { post(router, "/rpc/procedure.soQryFindOwners", &body).await }
        };
        run_cases("RPC", list, find_many).await
    }
}

/// One test, one container: a second container start in one binary races
/// rootless Docker's port manager (`tests/mcp_policy_pg.rs`).
#[tokio::test]
async fn a_request_can_never_filter_or_sort_by_a_server_only_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    for statement in SEED {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let mut failures = rest::cases(&test_pg.pool).await;
    failures.extend(rpc::cases(&test_pg.pool).await);
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
