use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_cbor::CborCodec;
use std::collections::BTreeMap;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/auth_engine.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AuthEngineAuthProvider;

#[derive(Clone)]
struct AuthEngineProcedures;

fn organization_scope(id: &str) -> Value {
    Value::Map(BTreeMap::from([(
        "id".to_owned(),
        Value::String(id.to_owned()),
    )]))
}

fn tenant_scope(id: &str) -> Value {
    Value::Map(BTreeMap::from([(
        "id".to_owned(),
        Value::String(id.to_owned()),
    )]))
}

impl AuthProvider for AuthEngineAuthProvider {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let mut fields = Vec::new();

        if let Some(value) = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
        {
            fields.push(("id".to_owned(), Value::String(value.to_owned())));
        }
        if let Some(value) = request
            .headers
            .get("x-user-id")
            .and_then(|value| value.to_str().ok())
        {
            fields.push(("userId".to_owned(), Value::String(value.to_owned())));
        }
        if let Some(value) = request
            .headers
            .get("x-role")
            .and_then(|value| value.to_str().ok())
        {
            fields.push(("role".to_owned(), Value::String(value.to_owned())));
        }
        if let Some(value) = request
            .headers
            .get("x-org-id")
            .and_then(|value| value.to_str().ok())
        {
            fields.push(("organization".to_owned(), organization_scope(value)));
        }
        if let Some(value) = request
            .headers
            .get("x-tenant-id")
            .and_then(|value| value.to_str().ok())
        {
            fields.push(("tenant".to_owned(), tenant_scope(value)));
        }
        if let Some(value) = request
            .headers
            .get("x-org-role")
            .and_then(|value| value.to_str().ok())
        {
            fields.push((
                "organizationRole".to_owned(),
                Value::String(value.to_owned()),
            ));
        }

        core::future::ready(Ok(if fields.is_empty() {
            CratestackContext::anonymous()
        } else {
            CratestackContext::authenticated(fields)
        }))
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for AuthEngineProcedures {
    async fn inspect_post(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::inspect_post::Args,
        _authorized: cratestack_schema::procedures::inspect_post::Authorized,
    ) -> Result<cratestack_schema::procedures::inspect_post::Output, cratestack::CratestackError>
    {
        Ok(cratestack_schema::EnginePost {
            id: args.args.postId,
            title: "Visible".to_owned(),
            published: true,
            authorId: "usr_1".to_owned(),
        })
    }

    async fn admin_pulse(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::admin_pulse::Args,
        _authorized: cratestack_schema::procedures::admin_pulse::Authorized,
    ) -> Result<cratestack_schema::procedures::admin_pulse::Output, cratestack::CratestackError>
    {
        Ok(cratestack_schema::EnginePost {
            id: args.args.postId,
            title: "Admin Pulse".to_owned(),
            published: true,
            authorId: "usr_2".to_owned(),
        })
    }
}

// The "non-owner should fail db-backed procedure auth" assertion below
// is not stale test data — it caught a REAL, confirmed authorization
// bypass. This is NOT "procedure-policy delegation drift"; the
// procedure-delegation wiring itself
// (`cratestack_schema::procedures::inspect_post::authorize_with_db` ->
// `db.engine_post().authorize_detail(id, ctx)`) is correct. The bug is
// one level down, in the SQL these `authorize_*`/scoped-read/scoped-
// write calls build.
//
// Root cause: `push_action_policy_query` in
// `crates/cratestack-sqlx/src/query/support/policy.rs` renders a
// model's allow policies as `A OR B OR ...` (one array element per
// separate `@@allow("<action>", ...)` attribute) with NO enclosing
// parentheses around the whole disjunction when the action has no
// matching `@@deny` clause (the `if !deny_policies.is_empty()` branch
// *does* wrap correctly via `NOT (...) AND (...)`; only the `else`
// branch — the common case, no `@@deny` — is missing the wrap). Every
// caller then splices that unwrapped string directly after `<row
// filter> AND `, e.g. in `authorize_record_action`
// (`crates/cratestack-sqlx/src/query/support/conditions.rs`):
// `... WHERE id = $1 AND {policy}`. Because SQL `AND` binds tighter
// than `OR`, `id = $1 AND A OR B` parses as `(id = $1 AND A) OR B` —
// the primary-key (or filter) scoping only binds to the FIRST allow
// clause; every other clause becomes a table-wide, row-blind OR.
//
// Confirmed live against Postgres (`EnginePost` in `auth_engine.cstack`
// declares two separate action clauses —
// `@@allow('all', auth() != null && auth().id == authorId)` and
// `@@allow('read', auth() != null && published)` — and no matching
// `@@deny`, which is exactly the trigger shape): calling
// `cool.engine_post().authorize_detail(id, ctx)` directly for
// `other_org_admin` (usr_4, wrong org/tenant, not the author) against
// `"this_id_does_not_exist"` — a primary key that is not in the table
// at all — still returns `Ok(())`, because `post_2` (seeded as
// `published = TRUE`, owned by a different user) satisfies the second
// OR-branch unconditionally and the broken WHERE clause never actually
// constrains by `id`. This is a full row-scoping bypass, not limited to
// this one procedure: the same `push_action_policy_query` combinator is
// used, unwrapped the same way, by every generated `find_unique`/
// `find_many` read (`push_scoped_conditions`) and every generated
// `update`/`delete`/`upsert`/batch write path
// (`query/write/*_exec.rs`, `query/batch/*.rs`) — any model+action with
// 2+ separate `@@allow` clauses and no `@@deny` for that action is
// exposed, on both the read-authorization and (for writes) the
// row-targeting side. This needs a real fix in
// `push_action_policy_query` (wrap the `else` branch's allow
// disjunction in the same way the deny-present branch already does),
// not a test change — this is a confirmed defect, not stale test data.
#[tokio::test]
// STATUS 2026-08-05: the SQL operator-precedence authorization bypass that
// originally blocked this test is FIXED (`push_action_policy_query` in
// `crates/cratestack-sqlx/src/query/support/policy.rs` now parenthesizes its
// whole predicate; pinned by `cratestack-sqlx`'s `tests_policy_precedence_bug`).
// Verified against real Postgres: every cross-tenant isolation assertion above
// line 425 now passes.
//
// A SECOND defect that was exposed at line 462+ has also been fixed:
// `Todo.organizationId String? @default(auth().organization.id)` was silently
// resolving to NULL when the caller's auth context omitted the nested
// `organization` claim, instead of failing validation. The `auth SessionUser`
// block declares `organization OrganizationScope` as required (non-optional),
// so a context missing it should be rejected. This enforces the invariant
// that required auth fields cannot be silently absent, preventing NULL values
// in tenant-scoping columns that bypass policy predicates (since `NULL != X`
// returns NULL in SQL, not true). This is now fixed in cratestack-sqlx's
// `resolve_default_value()` — it checks `CreateDefault::auth_field_required`
// and fails with `CratestackError::Validation` when a required auth field is missing.
async fn db_backed_auth_engine_supports_all_deny_and_auth_defaults() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    cratestack::sqlx::query("DROP TABLE IF EXISTS engine_posts, todos, scoped_notes")
        .execute(pool)
        .await
        .expect("auth engine test tables should reset");
    cratestack::sqlx::query("DROP TABLE IF EXISTS admin_panels")
        .execute(pool)
        .await
        .expect("auth engine test tables should reset");
    cratestack::sqlx::query(
        "CREATE TABLE engine_posts (id TEXT PRIMARY KEY DEFAULT ('post_' || md5(random()::text)), title TEXT NOT NULL, published BOOLEAN NOT NULL, author_id TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("posts table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE todos (id TEXT PRIMARY KEY DEFAULT ('todo_' || md5(random()::text)), owner_id TEXT NOT NULL, title TEXT NOT NULL, organization_id TEXT)",
    )
    .execute(pool)
    .await
    .expect("todos table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE scoped_notes (id TEXT PRIMARY KEY DEFAULT ('note_' || md5(random()::text)), owner_id TEXT NOT NULL, body TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("scoped_notes table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE admin_panels (id TEXT PRIMARY KEY DEFAULT ('panel_' || md5(random()::text)), title TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("admin_panels table should exist");
    cratestack::sqlx::query(
        "INSERT INTO engine_posts (id, title, published, author_id) VALUES ('post_1', 'Draft', FALSE, 'usr_1'), ('post_2', 'Published', TRUE, 'usr_2')",
    )
    .execute(pool)
    .await
    .expect("posts should seed");
    cratestack::sqlx::query(
        "INSERT INTO todos (id, owner_id, title, organization_id) VALUES ('todo_seed', 'usr_3', 'Existing Todo', 'org_2')",
    )
    .execute(pool)
    .await
    .expect("todos should seed");
    cratestack::sqlx::query(
        "INSERT INTO admin_panels (id, title) VALUES ('panel_1', 'Operations')",
    )
    .execute(pool)
    .await
    .expect("admin panels should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let owner = CratestackContext::authenticated([
        ("id".to_owned(), Value::String("usr_1".to_owned())),
        ("userId".to_owned(), Value::String("usr_1".to_owned())),
        ("organization".to_owned(), organization_scope("org_1")),
        ("tenant".to_owned(), tenant_scope("tenant_1")),
        ("role".to_owned(), Value::String("member".to_owned())),
        (
            "organizationRole".to_owned(),
            Value::String("member".to_owned()),
        ),
    ]);
    let org_admin = CratestackContext::authenticated([
        ("id".to_owned(), Value::String("usr_2".to_owned())),
        ("userId".to_owned(), Value::String("usr_2".to_owned())),
        ("organization".to_owned(), organization_scope("org_1")),
        ("tenant".to_owned(), tenant_scope("tenant_1")),
        ("role".to_owned(), Value::String("admin".to_owned())),
        (
            "organizationRole".to_owned(),
            Value::String("admin".to_owned()),
        ),
    ]);
    let other_org_admin = CratestackContext::authenticated([
        ("id".to_owned(), Value::String("usr_4".to_owned())),
        ("userId".to_owned(), Value::String("usr_4".to_owned())),
        ("organization".to_owned(), organization_scope("org_2")),
        ("tenant".to_owned(), tenant_scope("tenant_2")),
        ("role".to_owned(), Value::String("admin".to_owned())),
        (
            "organizationRole".to_owned(),
            Value::String("admin".to_owned()),
        ),
    ]);
    let anonymous = CratestackContext::anonymous();

    let owner_post = cool
        .engine_post()
        .find_unique("post_1".to_owned())
        .run(&owner)
        .await
        .expect("owner post read should succeed")
        .expect("owner post should be visible");
    assert_eq!(owner_post.id, "post_1");

    let published_post = cool
        .engine_post()
        .find_unique("post_2".to_owned())
        .run(&owner)
        .await
        .expect("published post read should succeed")
        .expect("published post should be visible");
    assert_eq!(published_post.id, "post_2");

    let anonymous_post = cool
        .engine_post()
        .find_unique("post_2".to_owned())
        .run(&anonymous)
        .await
        .expect("anonymous read should scope cleanly");
    assert!(anonymous_post.is_none());

    let allowed_admin_panel = cool
        .admin_panel()
        .find_unique("panel_1".to_owned())
        .run(&org_admin)
        .await
        .expect("same-tenant admin panel read should succeed")
        .expect("same-tenant admin panel should be visible");
    assert_eq!(allowed_admin_panel.title, "Operations");

    let non_admin_panel = cool
        .admin_panel()
        .find_unique("panel_1".to_owned())
        .run(&owner)
        .await
        .expect("non-admin panel read should scope cleanly");
    assert!(non_admin_panel.is_none());

    let wrong_tenant_panel = cool
        .admin_panel()
        .find_unique("panel_1".to_owned())
        .run(&other_org_admin)
        .await
        .expect("wrong-tenant panel read should scope cleanly");
    assert!(wrong_tenant_panel.is_none());

    cratestack_schema::procedures::inspect_post::authorize_with_db(
        &cool,
        &cratestack_schema::procedures::inspect_post::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_1".to_owned(),
            },
        },
        &owner,
    )
    .await
    .expect("owner should pass db-backed procedure auth");

    let hidden_post_error = cratestack_schema::procedures::inspect_post::authorize_with_db(
        &cool,
        &cratestack_schema::procedures::inspect_post::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_1".to_owned(),
            },
        },
        &other_org_admin,
    )
    .await
    .expect_err("non-owner should fail db-backed procedure auth");
    assert!(matches!(hidden_post_error, CratestackError::Forbidden(_)));

    cratestack_schema::procedures::admin_pulse::authorize(
        &cratestack_schema::procedures::admin_pulse::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_2".to_owned(),
            },
        },
        &org_admin,
    )
    .expect("same-tenant admin should pass built-in procedure auth");

    let wrong_tenant_pulse = cratestack_schema::procedures::admin_pulse::authorize(
        &cratestack_schema::procedures::admin_pulse::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_2".to_owned(),
            },
        },
        &other_org_admin,
    )
    .expect_err("wrong-tenant admin should fail built-in procedure auth");
    assert!(matches!(wrong_tenant_pulse, CratestackError::Forbidden(_)));

    let denied_create = cool
        .engine_post()
        .create(cratestack_schema::CreateEnginePostInput {
            title: "Wrong Author".to_owned(),
            published: false,
            authorId: "usr_2".to_owned(),
        })
        .run(&owner)
        .await
        .expect_err("mismatched author create should fail");
    assert!(matches!(denied_create, CratestackError::Forbidden(_)));

    let created_todo = cool
        .todo()
        .create(cratestack_schema::CreateTodoInput {
            ownerId: "usr_1".to_owned(),
            title: "Plan rollout".to_owned(),
        })
        .run(&owner)
        .await
        .expect("todo create should apply auth default and allow owner");
    assert_eq!(created_todo.organizationId.as_deref(), Some("org_1"));
    let created_todo_id = created_todo.id.clone();

    let updated_todo = cool
        .todo()
        .update(created_todo_id.clone())
        .set(cratestack_schema::UpdateTodoInput {
            ownerId: None,
            title: Some("Plan rollout now".to_owned()),
            organizationId: None,
        })
        .run(&org_admin)
        .await
        .expect("org admin in same org should update todo");
    assert_eq!(updated_todo.title, "Plan rollout now");

    let other_org_read = cool
        .todo()
        .find_unique(created_todo_id.clone())
        .run(&other_org_admin)
        .await
        .expect("other org read should scope cleanly");
    assert!(other_org_read.is_none());

    let other_org_update = cool
        .todo()
        .update(created_todo_id.clone())
        .set(cratestack_schema::UpdateTodoInput {
            ownerId: None,
            title: Some("Blocked".to_owned()),
            organizationId: None,
        })
        .run(&other_org_admin)
        .await
        .expect_err("other org admin update should fail");
    assert!(matches!(other_org_update, CratestackError::Forbidden(_)));

    let anonymous_note_create = cool
        .scoped_note()
        .create(cratestack_schema::CreateScopedNoteInput {
            body: "Blocked note".to_owned(),
        })
        .run(&anonymous)
        .await
        .expect_err("anonymous scoped note create should fail cleanly");
    assert!(matches!(
        anonymous_note_create,
        CratestackError::Forbidden(_)
    ));

    let created_note = cool
        .scoped_note()
        .create(cratestack_schema::CreateScopedNoteInput {
            body: "Owned note".to_owned(),
        })
        .run(&owner)
        .await
        .expect("authenticated scoped note create should apply owner default");
    assert_eq!(created_note.ownerId, "usr_1");

    let missing_org_error = cool
        .todo()
        .create(cratestack_schema::CreateTodoInput {
            ownerId: "usr_1".to_owned(),
            title: "Missing org".to_owned(),
        })
        .run(&CratestackContext::authenticated([
            ("id".to_owned(), Value::String("usr_1".to_owned())),
            ("userId".to_owned(), Value::String("usr_1".to_owned())),
            (
                "organizationRole".to_owned(),
                Value::String("member".to_owned()),
            ),
        ]))
        .await
        .expect_err("missing nested organization auth field should fail validation");
    assert!(matches!(missing_org_error, CratestackError::Validation(_)));

    let wrong_type_error = cool
        .scoped_note()
        .create(cratestack_schema::CreateScopedNoteInput {
            body: "Wrong type".to_owned(),
        })
        .run(&CratestackContext::authenticated([
            ("id".to_owned(), Value::String("usr_1".to_owned())),
            ("userId".to_owned(), Value::Int(1)),
        ]))
        .await
        .expect_err("wrong auth default type should fail validation");
    assert!(matches!(wrong_type_error, CratestackError::Validation(_)));

    let codec = CborCodec;
    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        codec.clone(),
        AuthEngineAuthProvider,
    );
    let procedure_router = cratestack_schema::axum::procedure_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        AuthEngineProcedures,
        codec.clone(),
        AuthEngineAuthProvider,
    );

    let same_org_get = router
        .clone()
        .oneshot(
            Request::get(format!("/todos/{created_todo_id}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_2")
                .header("x-user-id", "usr_2")
                .header("x-role", "admin")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("same-org get should complete");
    assert_eq!(same_org_get.status(), StatusCode::OK);

    let scoped_note_request_body = codec
        .encode(&cratestack_schema::CreateScopedNoteInput {
            body: "Created over HTTP".to_owned(),
        })
        .expect("scoped note body should encode");
    let scoped_note_create = router
        .clone()
        .oneshot(
            Request::post("/scoped_notes")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_1")
                .header("x-user-id", "usr_1")
                .header("x-role", "member")
                .header("x-tenant-id", "tenant_1")
                .body(Body::from(scoped_note_request_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("scoped note create should complete");
    assert_eq!(scoped_note_create.status(), StatusCode::CREATED);
    let scoped_note_response_body = to_bytes(scoped_note_create.into_body(), usize::MAX)
        .await
        .expect("scoped note create body should read");
    let scoped_note: cratestack_schema::ScopedNote = codec
        .decode(&scoped_note_response_body)
        .expect("scoped note create response should decode");
    assert_eq!(scoped_note.ownerId, "usr_1");

    let missing_user_claim = router
        .clone()
        .oneshot(
            Request::post("/scoped_notes")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_1")
                .header("x-role", "member")
                .header("x-tenant-id", "tenant_1")
                .body(Body::from(scoped_note_request_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("missing user claim request should complete");
    // `userId` is required in the `auth SessionUser` block, and
    // `ScopedNote.ownerId @default(auth().userId)` is a non-nullable
    // column — a caller missing it fails `resolve_default_value` with
    // `CratestackError::Validation`, which `cratestack-core`'s `IntoResponse`
    // maps to 422 (see `error.rs`), not 400. This assertion could not
    // be checked against real behavior before this PR un-ignored the
    // test (see `banking_validation.rs` for the same Validation -> 422
    // mapping pinned elsewhere).
    assert_eq!(
        missing_user_claim.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let other_org_get = router
        .clone()
        .oneshot(
            Request::get(format!("/todos/{created_todo_id}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_4")
                .header("x-user-id", "usr_4")
                .header("x-role", "admin")
                .header("x-org-id", "org_2")
                .header("x-tenant-id", "tenant_2")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("other-org get should complete");
    assert_eq!(other_org_get.status(), StatusCode::NOT_FOUND);

    let other_org_patch_body = codec
        .encode(&cratestack_schema::UpdateTodoInput {
            ownerId: None,
            title: Some("Blocked over HTTP".to_owned()),
            organizationId: None,
        })
        .expect("patch body should encode");
    let other_org_patch = router
        .clone()
        .oneshot(
            Request::patch(format!("/todos/{created_todo_id}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_4")
                .header("x-user-id", "usr_4")
                .header("x-role", "admin")
                .header("x-org-id", "org_2")
                .header("x-tenant-id", "tenant_2")
                .header("x-org-role", "admin")
                .body(Body::from(other_org_patch_body))
                .expect("request should build"),
        )
        .await
        .expect("other-org patch should complete");
    assert_eq!(other_org_patch.status(), StatusCode::FORBIDDEN);

    let same_org_delete = router
        .clone()
        .oneshot(
            Request::delete(format!("/todos/{created_todo_id}"))
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_2")
                .header("x-user-id", "usr_2")
                .header("x-role", "admin")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("same-org delete should complete");
    assert_eq!(same_org_delete.status(), StatusCode::OK);

    let forbidden_delete = router
        .clone()
        .oneshot(
            Request::delete("/todos/todo_seed")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_2")
                .header("x-user-id", "usr_2")
                .header("x-role", "admin")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("cross-org delete should complete");
    assert_eq!(forbidden_delete.status(), StatusCode::FORBIDDEN);
    let forbidden_body = to_bytes(forbidden_delete.into_body(), usize::MAX)
        .await
        .expect("forbidden delete body should read");
    let forbidden_error: cratestack::CratestackErrorResponse = codec
        .decode(&forbidden_body)
        .expect("forbidden delete should decode");
    assert_eq!(forbidden_error.code, "FORBIDDEN");

    let inspect_post_body = codec
        .encode(&cratestack_schema::procedures::inspect_post::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_1".to_owned(),
            },
        })
        .expect("inspect post body should encode");
    let inspect_post_allowed = procedure_router
        .clone()
        .oneshot(
            Request::post("/$procs/inspectPost")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_1")
                .header("x-user-id", "usr_1")
                .header("x-role", "member")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "member")
                .body(Body::from(inspect_post_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("inspect post allowed request should complete");
    assert_eq!(inspect_post_allowed.status(), StatusCode::OK);

    let inspect_post_denied = procedure_router
        .clone()
        .oneshot(
            Request::post("/$procs/inspectPost")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_4")
                .header("x-user-id", "usr_4")
                .header("x-role", "admin")
                .header("x-org-id", "org_2")
                .header("x-tenant-id", "tenant_2")
                .header("x-org-role", "admin")
                .body(Body::from(inspect_post_body))
                .expect("request should build"),
        )
        .await
        .expect("inspect post denied request should complete");
    assert_eq!(inspect_post_denied.status(), StatusCode::FORBIDDEN);

    let admin_pulse_body = codec
        .encode(&cratestack_schema::procedures::admin_pulse::Args {
            args: cratestack_schema::InspectPostInput {
                postId: "post_2".to_owned(),
            },
        })
        .expect("admin pulse body should encode");
    let admin_panel_allowed = router
        .clone()
        .oneshot(
            Request::get("/admin_panels/panel_1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_2")
                .header("x-user-id", "usr_2")
                .header("x-role", "admin")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("admin panel request should complete");
    assert_eq!(admin_panel_allowed.status(), StatusCode::OK);

    let admin_panel_denied = router
        .clone()
        .oneshot(
            Request::get("/admin_panels/panel_1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_4")
                .header("x-user-id", "usr_4")
                .header("x-role", "admin")
                .header("x-org-id", "org_2")
                .header("x-tenant-id", "tenant_2")
                .header("x-org-role", "admin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("admin panel denied request should complete");
    assert_eq!(admin_panel_denied.status(), StatusCode::NOT_FOUND);

    let admin_pulse_allowed = procedure_router
        .clone()
        .oneshot(
            Request::post("/$procs/adminPulse")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_2")
                .header("x-user-id", "usr_2")
                .header("x-role", "admin")
                .header("x-org-id", "org_1")
                .header("x-tenant-id", "tenant_1")
                .header("x-org-role", "admin")
                .body(Body::from(admin_pulse_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("admin pulse allowed request should complete");
    assert_eq!(admin_pulse_allowed.status(), StatusCode::OK);

    let admin_pulse_denied = procedure_router
        .oneshot(
            Request::post("/$procs/adminPulse")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "usr_4")
                .header("x-user-id", "usr_4")
                .header("x-role", "admin")
                .header("x-org-id", "org_2")
                .header("x-tenant-id", "tenant_2")
                .header("x-org-role", "admin")
                .body(Body::from(admin_pulse_body))
                .expect("request should build"),
        )
        .await
        .expect("admin pulse denied request should complete");
    assert_eq!(admin_pulse_denied.status(), StatusCode::FORBIDDEN);
}
