use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CoolCodec, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use std::collections::BTreeMap;
use tower::util::ServiceExt;

include_schema!("tests/fixtures/auth_engine.cstack");

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
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
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
            CoolContext::anonymous()
        } else {
            CoolContext::authenticated(fields)
        }))
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for AuthEngineProcedures {
    fn inspect_post(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::inspect_post::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::inspect_post::Output, cratestack::CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::Post {
                id: args.args.postId,
                title: "Visible".to_owned(),
                published: true,
                authorId: "usr_1".to_owned(),
            })
        }
    }

    fn admin_pulse(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::admin_pulse::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::admin_pulse::Output, cratestack::CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::Post {
                id: args.args.postId,
                title: "Admin Pulse".to_owned(),
                published: true,
                authorId: "usr_2".to_owned(),
            })
        }
    }
}

#[tokio::test]
async fn db_backed_auth_engine_supports_all_deny_and_auth_defaults() {
    let database_url = match std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return,
    };

    let pool = match PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
    {
        Ok(pool) => pool,
        Err(_) => return,
    };

    cratestack::sqlx::query("DROP TABLE IF EXISTS posts, todos, scoped_notes")
        .execute(&pool)
        .await
        .expect("auth engine test tables should reset");
    cratestack::sqlx::query("DROP TABLE IF EXISTS admin_panels")
        .execute(&pool)
        .await
        .expect("auth engine test tables should reset");
    cratestack::sqlx::query(
        "CREATE TABLE posts (id TEXT PRIMARY KEY DEFAULT ('post_' || md5(random()::text)), title TEXT NOT NULL, published BOOLEAN NOT NULL, author_id TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("posts table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE todos (id TEXT PRIMARY KEY DEFAULT ('todo_' || md5(random()::text)), owner_id TEXT NOT NULL, title TEXT NOT NULL, organization_id TEXT)",
    )
    .execute(&pool)
    .await
    .expect("todos table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE scoped_notes (id TEXT PRIMARY KEY DEFAULT ('note_' || md5(random()::text)), owner_id TEXT NOT NULL, body TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("scoped_notes table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE admin_panels (id TEXT PRIMARY KEY DEFAULT ('panel_' || md5(random()::text)), title TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("admin_panels table should exist");
    cratestack::sqlx::query(
        "INSERT INTO posts (id, title, published, author_id) VALUES ('post_1', 'Draft', FALSE, 'usr_1'), ('post_2', 'Published', TRUE, 'usr_2')",
    )
    .execute(&pool)
    .await
    .expect("posts should seed");
    cratestack::sqlx::query(
        "INSERT INTO todos (id, owner_id, title, organization_id) VALUES ('todo_seed', 'usr_3', 'Existing Todo', 'org_2')",
    )
    .execute(&pool)
    .await
    .expect("todos should seed");
    cratestack::sqlx::query(
        "INSERT INTO admin_panels (id, title) VALUES ('panel_1', 'Operations')",
    )
    .execute(&pool)
    .await
    .expect("admin panels should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let owner = CoolContext::authenticated([
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
    let org_admin = CoolContext::authenticated([
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
    let other_org_admin = CoolContext::authenticated([
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
    let anonymous = CoolContext::anonymous();

    let owner_post = cool
        .post()
        .find_unique("post_1".to_owned())
        .run(&owner)
        .await
        .expect("owner post read should succeed")
        .expect("owner post should be visible");
    assert_eq!(owner_post.id, "post_1");

    let published_post = cool
        .post()
        .find_unique("post_2".to_owned())
        .run(&owner)
        .await
        .expect("published post read should succeed")
        .expect("published post should be visible");
    assert_eq!(published_post.id, "post_2");

    let anonymous_post = cool
        .post()
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
    assert!(matches!(hidden_post_error, CoolError::Forbidden(_)));

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
    assert!(matches!(wrong_tenant_pulse, CoolError::Forbidden(_)));

    let denied_create = cool
        .post()
        .create(cratestack_schema::CreatePostInput {
            title: "Wrong Author".to_owned(),
            published: false,
            authorId: "usr_2".to_owned(),
        })
        .run(&owner)
        .await
        .expect_err("mismatched author create should fail");
    assert!(matches!(denied_create, CoolError::Forbidden(_)));

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
    assert!(matches!(other_org_update, CoolError::Forbidden(_)));

    let anonymous_note_create = cool
        .scoped_note()
        .create(cratestack_schema::CreateScopedNoteInput {
            body: "Blocked note".to_owned(),
        })
        .run(&anonymous)
        .await
        .expect_err("anonymous scoped note create should fail cleanly");
    assert!(matches!(anonymous_note_create, CoolError::Forbidden(_)));

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
        .run(&CoolContext::authenticated([
            ("id".to_owned(), Value::String("usr_1".to_owned())),
            ("userId".to_owned(), Value::String("usr_1".to_owned())),
            (
                "organizationRole".to_owned(),
                Value::String("member".to_owned()),
            ),
        ]))
        .await
        .expect_err("missing nested organization auth field should fail validation");
    assert!(matches!(missing_org_error, CoolError::Validation(_)));

    let wrong_type_error = cool
        .scoped_note()
        .create(cratestack_schema::CreateScopedNoteInput {
            body: "Wrong type".to_owned(),
        })
        .run(&CoolContext::authenticated([
            ("id".to_owned(), Value::String("usr_1".to_owned())),
            ("userId".to_owned(), Value::Int(1)),
        ]))
        .await
        .expect_err("wrong auth default type should fail validation");
    assert!(matches!(wrong_type_error, CoolError::Validation(_)));

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
    assert_eq!(missing_user_claim.status(), StatusCode::BAD_REQUEST);

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
    let forbidden_error: cratestack::CoolErrorResponse = codec
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
