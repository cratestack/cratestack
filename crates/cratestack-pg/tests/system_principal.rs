//! PG-backed coverage for `auth().isSystem()` (issue #486 / ADR 0038
//! blocker B1).
//!
//! Everything in `cratestack-sqlx/src/tests_system_principal_policy.rs`
//! is a unit test against the SQL a policy renders to; nothing there
//! proves a system read/write actually resolves against a real table.
//! `render_policy_predicate` (the file the task names explicitly) only
//! participates in the `find_unique`/list read path, so the read half
//! of this feature — the half route suppression could never have fixed
//! — needs a real database to be believable. These tests supply that.
//!
//! Five things are proven here, matching the acceptance criteria:
//! 1. A system caller is permitted where a policy names `isSystem()`,
//!    for both read (`list`/`detail`) and write (`create`/`update`/
//!    `delete`).
//! 2. A system caller is DENIED on a model that never names
//!    `isSystem()` — the fail-closed proof
//!    (`system_caller_is_denied_on_a_model_that_never_names_is_system`).
//! 3. A non-system caller's existing behavior is unaffected by a policy
//!    gaining an `isSystem()` arm
//!    (`non_system_owner_is_unaffected_by_the_is_system_arm`).
//! 4. A system write is auditable: it lands in `cratestack_audit` with
//!    an actor id of `system:<service>`
//!    (`system_write_is_captured_in_the_audit_trail`).
//! 5. Nothing an HTTP caller controls can produce a system context —
//!    the forgery boundary
//!    (`http_request_cannot_produce_a_system_context`).

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{AuthProvider, CoolContext, RequestContext, SystemContext, Value};
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/system_principal.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query(
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, \
         system_devices, owner_only_notes",
    )
    .execute(pool)
    .await
    .expect("drop tables");
    query(
        "CREATE TABLE system_devices (
            id BIGINT PRIMARY KEY,
            subject_id TEXT NOT NULL,
            label TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create system_devices table");
    query(
        "CREATE TABLE owner_only_notes (
            id BIGINT PRIMARY KEY,
            subject_id TEXT NOT NULL,
            body TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create owner_only_notes table");
}

fn system_ctx() -> CoolContext {
    SystemContext::for_service("device-reconciler").into_context()
}

fn owner_ctx(subject_id: &str) -> CoolContext {
    CoolContext::authenticated([("subjectId".to_owned(), Value::String(subject_id.to_owned()))])
}

/// The headline case: a policy that names `isSystem()` grants the
/// system principal on every action it's named in — read and write
/// alike, proving design constraint #5 (must work for both).
#[tokio::test]
async fn system_caller_is_permitted_where_the_policy_names_is_system() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let system = system_ctx();

    // CREATE: `@@allow("create", auth().isSystem())` — no owner branch
    // at all, so this is system-only by construction.
    let created = cool
        .system_device()
        .create(cratestack_schema::CreateSystemDeviceInput {
            id: 1,
            subjectId: "owner-a".to_owned(),
            label: "reconciled-device".to_owned(),
        })
        .run(&system)
        .await
        .expect("system create should be allowed");
    assert_eq!(created.subjectId, "owner-a");

    // Seed a second row belonging to a *different* owner directly, so
    // the read assertions below prove the system caller sees rows it
    // does not own — the property route suppression could never give
    // you, because a suppressed route still returns nothing at all.
    query("INSERT INTO system_devices (id, subject_id, label) VALUES (2, 'owner-b', 'other')")
        .execute(pool)
        .await
        .expect("seed second device");

    // LIST: system sees both rows regardless of subjectId.
    let listed = cool
        .system_device()
        .find_many()
        .run(&system)
        .await
        .expect("system list should be allowed");
    let mut ids: Vec<i64> = listed.iter().map(|d| d.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 2], "system caller should see every row");

    // DETAIL: system can read a row it does not own.
    let detail = cool
        .system_device()
        .find_unique(2)
        .run(&system)
        .await
        .expect("system detail read should succeed")
        .expect("row should be visible to a system caller");
    assert_eq!(detail.subjectId, "owner-b");

    // UPDATE: system can update a row it does not own.
    let updated = cool
        .system_device()
        .update(2)
        .set(cratestack_schema::UpdateSystemDeviceInput {
            subjectId: None,
            label: Some("reconciled-by-system".to_owned()),
        })
        .run(&system)
        .await
        .expect("system update should be allowed");
    assert_eq!(updated.label, "reconciled-by-system");

    // DELETE: `@@allow("delete", auth().isSystem())` — system-only, no
    // owner branch.
    cool.system_device()
        .delete(1)
        .run(&system)
        .await
        .expect("system delete should be allowed");
    let remaining = cool
        .system_device()
        .find_many()
        .run(&system)
        .await
        .expect("list after delete");
    assert_eq!(remaining.len(), 1);
}

/// A non-system caller's existing owner-scoped behavior must not change
/// just because the schema also names `isSystem()` in the same clause.
/// This is the regression the `||` composition exists to avoid: adding
/// system access must not implicitly grant or take away anything from
/// an ordinary caller.
#[tokio::test]
async fn non_system_owner_is_unaffected_by_the_is_system_arm() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    query(
        "INSERT INTO system_devices (id, subject_id, label) VALUES \
         (1, 'owner-a', 'mine'), (2, 'owner-b', 'not-mine')",
    )
    .execute(pool)
    .await
    .expect("seed devices");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let owner_a = owner_ctx("owner-a");

    // LIST scoping is unchanged: an owner sees only their own row.
    let listed = cool
        .system_device()
        .find_many()
        .run(&owner_a)
        .await
        .expect("owner list should succeed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].subjectId, "owner-a");

    // DETAIL on someone else's row is still invisible.
    let other = cool
        .system_device()
        .find_unique(2)
        .run(&owner_a)
        .await
        .expect("owner detail read should scope cleanly");
    assert!(other.is_none(), "owner must not see another owner's row");

    // UPDATE on their own row still works via the ownership branch.
    let updated = cool
        .system_device()
        .update(1)
        .set(cratestack_schema::UpdateSystemDeviceInput {
            subjectId: None,
            label: Some("relabeled-by-owner".to_owned()),
        })
        .run(&owner_a)
        .await
        .expect("owner update on their own row should succeed");
    assert_eq!(updated.label, "relabeled-by-owner");

    // UPDATE on someone else's row is still forbidden.
    let denied = cool
        .system_device()
        .update(2)
        .set(cratestack_schema::UpdateSystemDeviceInput {
            subjectId: None,
            label: Some("hijacked".to_owned()),
        })
        .run(&owner_a)
        .await;
    assert!(
        denied.is_err(),
        "owner must not be able to update another owner's row"
    );

    // CREATE and DELETE were never granted to owners at all — only to
    // `isSystem()` — so both must still fail for a non-system caller.
    let create_denied = cool
        .system_device()
        .create(cratestack_schema::CreateSystemDeviceInput {
            id: 3,
            subjectId: "owner-a".to_owned(),
            label: "self-service".to_owned(),
        })
        .run(&owner_a)
        .await;
    assert!(
        create_denied.is_err(),
        "an ordinary caller must not gain create access"
    );

    let delete_denied = cool.system_device().delete(1).run(&owner_a).await;
    assert!(
        delete_denied.is_err(),
        "an ordinary caller must not gain delete access"
    );
}

/// THE FAIL-CLOSED PROOF (design constraint #2). `OwnerOnlyNote` never
/// names `isSystem()` in any policy. A system caller must get exactly
/// the same nothing an unrelated, claim-less caller would get — not a
/// bypass, not partial access, nothing.
#[tokio::test]
async fn system_caller_is_denied_on_a_model_that_never_names_is_system() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    query("INSERT INTO owner_only_notes (id, subject_id, body) VALUES (1, 'owner-a', 'secret')")
        .execute(pool)
        .await
        .expect("seed note");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let system = system_ctx();

    let listed = cool
        .owner_only_note()
        .find_many()
        .run(&system)
        .await
        .expect("list should still execute, just scoped to nothing");
    assert!(
        listed.is_empty(),
        "a system caller must see zero rows on a model that never names isSystem()"
    );

    let detail = cool
        .owner_only_note()
        .find_unique(1)
        .run(&system)
        .await
        .expect("detail read should execute");
    assert!(
        detail.is_none(),
        "a system caller must not be able to read a row on a model that never names isSystem()"
    );

    let update_result = cool
        .owner_only_note()
        .update(1)
        .set(cratestack_schema::UpdateOwnerOnlyNoteInput {
            subjectId: None,
            body: Some("overwritten by system".to_owned()),
        })
        .run(&system)
        .await;
    assert!(
        update_result.is_err(),
        "a system caller must not be able to write a row on a model that never names isSystem()"
    );

    // Contrast: the real owner can still do all of this — the FALSE
    // above is the policy denying the system caller specifically, not
    // the policy or the fixture being broken.
    let owner_a = owner_ctx("owner-a");
    let owner_listed = cool
        .owner_only_note()
        .find_many()
        .run(&owner_a)
        .await
        .expect("owner list should succeed");
    assert_eq!(owner_listed.len(), 1);
}

/// Design constraint #3 (auditable). `SystemDevice` carries `@@audit`;
/// a system-attributed create must land a `cratestack_audit` row whose
/// `actor.id` is `system:<service>` and whose claims include the
/// service name — see `SystemContext::for_service`'s doc comment for
/// why that shape flows through the existing audit path unchanged.
#[tokio::test]
async fn system_write_is_captured_in_the_audit_trail() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let system = SystemContext::for_service("device-reconciler");

    cool.system_device()
        .create(cratestack_schema::CreateSystemDeviceInput {
            id: 1,
            subjectId: "owner-a".to_owned(),
            label: "audited-device".to_owned(),
        })
        .run(system.context())
        .await
        .expect("system create should be allowed and audited");

    let rows = query("SELECT model, operation, actor FROM cratestack_audit ORDER BY occurred_at")
        .fetch_all(pool)
        .await
        .expect("fetch audit rows");
    assert_eq!(rows.len(), 1, "expected exactly one audit row");

    let model: String = rows[0].get("model");
    assert_eq!(model, "SystemDevice");
    let operation: String = rows[0].get("operation");
    assert_eq!(operation, "create");

    let actor: serde_json::Value = rows[0].get("actor");
    assert_eq!(
        actor["id"],
        serde_json::json!("system:device-reconciler"),
        "a system write's actor id should be attributed to the service, not left blank"
    );
    // `AuditActor::claims` is `BTreeMap<String, cratestack_core::Value>`.
    // This used to assert `{"String": "device-reconciler"}`: `Value`
    // derived serde's externally-tagged enum representation, so every
    // audit claim was persisted wrapped in its own variant name. That
    // derive is gone — `Value` now serializes untagged, matching the
    // shape it already persisted through `to_plain_json` — so a claim
    // lands as the bare JSON string it always should have been.
    assert_eq!(
        actor["claims"]["service"],
        serde_json::json!("device-reconciler"),
        "the service claim should survive into the audit actor's claims snapshot"
    );
}

#[derive(Clone)]
struct SystemPrincipalAuthProvider;

impl AuthProvider for SystemPrincipalAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let subject_id = request
            .headers
            .get("x-subject-id")
            .and_then(|value| value.to_str().ok());

        // A plausible, real-world integration mistake: naively
        // forwarding a client-supplied "I am the system" signal into
        // the context as an ordinary claim. This must still not reach
        // `CoolContext::is_system()` — that flag has no public setter
        // at all, so this claim is inert no matter what name it uses.
        // If this ever started working, it would mean the forgery
        // boundary had been broken at the type level, not just at this
        // one call site.
        let forged_system_claim = request
            .headers
            .get("x-claims-system")
            .and_then(|value| value.to_str().ok())
            .map(|value| value == "true");

        let ctx = match subject_id {
            Some(subject_id) => {
                let mut fields =
                    vec![("subjectId".to_owned(), Value::String(subject_id.to_owned()))];
                if let Some(claimed) = forged_system_claim {
                    fields.push(("system".to_owned(), Value::Bool(claimed)));
                }
                CoolContext::authenticated(fields)
            }
            None => CoolContext::anonymous(),
        };

        core::future::ready(Ok(ctx))
    }
}

/// THE FORGERY PROOF (design constraint #4). There is no header, no
/// claim, and no request-controlled path that can make
/// `CoolContext::is_system()` return `true`. This drives a *real*
/// generated axum router — not a hand-called function — so the whole
/// request pipeline (`AuthProvider::authenticate` ->
/// `enrich_context_from_headers` -> the generated handler -> the ORM)
/// participates, not just the policy evaluator in isolation.
#[tokio::test]
async fn http_request_cannot_produce_a_system_context() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    query("INSERT INTO system_devices (id, subject_id, label) VALUES (1, 'owner-a', 'mine'), (2, 'owner-b', 'not-mine')")
        .execute(pool)
        .await
        .expect("seed devices");

    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        cratestack_codec_json::JsonCodec,
        SystemPrincipalAuthProvider,
    );

    // A request that tries to smuggle a system claim in over HTTP,
    // requesting a row it does not own. If the forgery boundary held,
    // this resolves exactly like any other cross-owner read: not found
    // (the generated read handler maps an empty policy-scoped read to
    // 404, matching every other owner-scoped fixture in this suite).
    let response = router
        .oneshot(
            Request::get("/system_devices/2")
                .header("accept", "application/json")
                .header("x-subject-id", "owner-a")
                .header("x-claims-system", "true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a forged system claim over HTTP must not unlock another owner's row"
    );
}
