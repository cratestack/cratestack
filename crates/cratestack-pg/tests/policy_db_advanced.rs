use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CoolCodec, CoolContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/advanced_policy.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AdvancedPolicyAuthProvider;

impl AuthProvider for AdvancedPolicyAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let mut fields = Vec::new();

        if let Some(id) = request.headers.get("x-auth-id") {
            let id = match id.to_str() {
                Ok(id) => id,
                Err(error) => {
                    return core::future::ready(Err(cratestack::CoolError::BadRequest(
                        error.to_string(),
                    )));
                }
            };
            let id = match id.parse::<i64>() {
                Ok(id) => id,
                Err(error) => {
                    return core::future::ready(Err(cratestack::CoolError::BadRequest(
                        error.to_string(),
                    )));
                }
            };
            fields.push(("id".to_owned(), Value::Int(id)));
        }

        if let Some(role) = request.headers.get("x-role") {
            let role = match role.to_str() {
                Ok(role) => role,
                Err(error) => {
                    return core::future::ready(Err(cratestack::CoolError::BadRequest(
                        error.to_string(),
                    )));
                }
            };
            fields.push(("role".to_owned(), Value::String(role.to_owned())));
        }

        if let Some(email) = request.headers.get("x-email") {
            let email = match email.to_str() {
                Ok(email) => email,
                Err(error) => {
                    return core::future::ready(Err(cratestack::CoolError::BadRequest(
                        error.to_string(),
                    )));
                }
            };
            fields.push(("email".to_owned(), Value::String(email.to_owned())));
        }

        core::future::ready(Ok(if fields.is_empty() {
            CoolContext::anonymous()
        } else {
            CoolContext::authenticated(fields)
        }))
    }
}

// This test used to publish post 1 via `owner_admin` and then assert
// `other_admin` could NOT read it. The advanced schema's
// `@@allow("read", auth() != null && published)` allows any
// authenticated caller to read a published row, so that assertion
// contradicted its own setup (confirmed: `other_admin` genuinely CAN
// read a published post under this policy). Fixed (2026-08 audit) per
// option (a) from the original note — split the "publish makes a post
// visible to everyone" behavior (still exercised against post 1) from
// the "non-owner cannot read another user's still-draft post" behavior
// (now exercised against post 4, `'Owner Only Draft'`, which is never
// published in this test) so both real policy properties get checked
// without contradicting each other.
#[tokio::test]
async fn db_backed_advanced_policy_enforcement() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;

    cratestack::sqlx::query("DROP TABLE IF EXISTS advanced_posts, advanced_users CASCADE")
        .execute(pool)
        .await
        .expect("drop stale tables");
    cratestack::sqlx::query(
        "CREATE TABLE advanced_users (id BIGINT PRIMARY KEY, email TEXT NOT NULL, banned BOOLEAN NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("advanced_users table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE advanced_posts (id BIGINT PRIMARY KEY, title TEXT NOT NULL, published BOOLEAN NOT NULL, author_id BIGINT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("advanced_posts table should exist");
    cratestack::sqlx::query("TRUNCATE TABLE advanced_posts, advanced_users")
        .execute(pool)
        .await
        .expect("tables should truncate");
    cratestack::sqlx::query(
        "INSERT INTO advanced_users (id, email, banned) VALUES (1, 'owner@example.com', FALSE), (2, 'other@example.com', FALSE), (3, 'blocked@example.com', TRUE)",
    )
    .execute(pool)
    .await
    .expect("advanced_users should seed");
    cratestack::sqlx::query(
        "INSERT INTO advanced_posts (id, title, published, author_id) VALUES (1, 'Draft', FALSE, 1), (2, 'Other Draft', FALSE, 2), (3, 'Blocked Published', TRUE, 3), (4, 'Owner Only Draft', FALSE, 1)",
    )
    .execute(pool)
    .await
    .expect("advanced_posts should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let owner_admin = CoolContext::authenticated([
        ("id".to_owned(), Value::Int(1)),
        ("role".to_owned(), Value::String("admin".to_owned())),
        (
            "email".to_owned(),
            Value::String("owner@example.com".to_owned()),
        ),
    ]);
    let owner_member = CoolContext::authenticated([
        ("id".to_owned(), Value::Int(1)),
        ("role".to_owned(), Value::String("member".to_owned())),
        (
            "email".to_owned(),
            Value::String("owner@example.com".to_owned()),
        ),
    ]);
    let other_admin = CoolContext::authenticated([
        ("id".to_owned(), Value::Int(2)),
        ("role".to_owned(), Value::String("admin".to_owned())),
        (
            "email".to_owned(),
            Value::String("other@example.com".to_owned()),
        ),
    ]);
    let anonymous = CoolContext::anonymous();

    let updated = cool
        .advanced_post()
        .update(1_i64)
        .set(cratestack_schema::UpdateAdvancedPostInput {
            title: Some("Updated By Owner Admin".to_owned()),
            published: Some(true),
            authorId: None,
        })
        .run(&owner_admin)
        .await
        .expect("owner admin update should succeed");
    assert_eq!(updated.title, "Updated By Owner Admin");

    let owner_read = cool
        .advanced_post()
        .find_unique(1_i64)
        .run(&owner_member)
        .await
        .expect("owner read should scope cleanly")
        .expect("owner should see their own post through the email relation policy");
    assert_eq!(owner_read.id, 1);

    // Post 1 was just published above, so the `@@allow("read", auth() !=
    // null && published)` clause now grants read access to any
    // authenticated caller, not just the owner — this asserts that
    // clause actually works, rather than (incorrectly) asserting a
    // non-owner still can't see it.
    let other_read_published = cool
        .advanced_post()
        .find_unique(1_i64)
        .run(&other_admin)
        .await
        .expect("published post read should scope cleanly")
        .expect("any authenticated caller should see a published post");
    assert_eq!(other_read_published.id, 1);

    // Post 4 is never published in this test, so it stays gated by the
    // owner-only `@@allow("read", author.email == auth().email)` clause
    // — this is the actual "non-owner can't read another user's draft"
    // property the original (contradictory) assertion was meant to
    // cover.
    let other_read_draft = cool
        .advanced_post()
        .find_unique(4_i64)
        .run(&other_admin)
        .await
        .expect("non-owner draft read should scope cleanly");
    assert!(other_read_draft.is_none());

    let owner_draft_read_direct = cool
        .advanced_post()
        .find_unique(4_i64)
        .run(&owner_admin)
        .await
        .expect("owner draft read should scope cleanly")
        .expect("owner should see their own draft through the email relation policy");
    assert_eq!(owner_draft_read_direct.id, 4);

    let blocked_read = cool
        .advanced_post()
        .find_unique(3_i64)
        .run(&owner_admin)
        .await
        .expect("blocked author read should scope cleanly");
    assert!(blocked_read.is_none());

    let owner_member_error = cool
        .advanced_post()
        .update(1_i64)
        .set(cratestack_schema::UpdateAdvancedPostInput {
            title: Some("Blocked Member".to_owned()),
            published: None,
            authorId: None,
        })
        .run(&owner_member)
        .await
        .expect_err("owner member update should fail");
    assert!(matches!(
        owner_member_error,
        cratestack::CoolError::Forbidden(_)
    ));

    let other_admin_error = cool
        .advanced_post()
        .update(1_i64)
        .set(cratestack_schema::UpdateAdvancedPostInput {
            title: Some("Blocked Other Admin".to_owned()),
            published: None,
            authorId: None,
        })
        .run(&other_admin)
        .await
        .expect_err("non-owner admin update should fail");
    assert!(matches!(
        other_admin_error,
        cratestack::CoolError::Forbidden(_)
    ));

    let anonymous_error = cool
        .advanced_post()
        .update(1_i64)
        .set(cratestack_schema::UpdateAdvancedPostInput {
            title: Some("Blocked Anonymous".to_owned()),
            published: None,
            authorId: None,
        })
        .run(&anonymous)
        .await
        .expect_err("anonymous update should fail");
    assert!(matches!(
        anonymous_error,
        cratestack::CoolError::Forbidden(_)
    ));

    let router = cratestack_schema::axum::model_router(cool, CborCodec, AdvancedPolicyAuthProvider);
    let codec = CborCodec;
    let body = codec
        .encode(&cratestack_schema::UpdateAdvancedPostInput {
            title: Some("Updated Through Route".to_owned()),
            published: Some(true),
            authorId: None,
        })
        .expect("request should encode");

    let allowed = router
        .clone()
        .oneshot(
            Request::patch("/advanced_posts/1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .header("x-role", "admin")
                .header("x-email", "owner@example.com")
                .body(Body::from(body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");
    assert_eq!(allowed.status(), StatusCode::OK);

    let denied = router
        .clone()
        .oneshot(
            Request::patch("/advanced_posts/1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .header("x-role", "member")
                .header("x-email", "owner@example.com")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let denied_body = to_bytes(denied.into_body(), usize::MAX)
        .await
        .expect("response body should decode");
    let denied_error: cratestack::CoolErrorResponse = codec
        .decode(&denied_body)
        .expect("forbidden error should decode");
    assert_eq!(denied_error.code, "FORBIDDEN");

    let owner_read = router
        .clone()
        .oneshot(
            Request::get("/advanced_posts/1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .header("x-role", "member")
                .header("x-email", "owner@example.com")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("owner read request should complete");
    assert_eq!(owner_read.status(), StatusCode::OK);

    // Post 1 was published above (see the `allowed` PATCH request), so
    // `@@allow("read", auth() != null && published)` now grants any
    // authenticated caller read access — asserting the opposite here
    // would just be re-testing the same setup contradiction the
    // `find_unique` checks above used to have.
    let other_published_read = router
        .clone()
        .oneshot(
            Request::get("/advanced_posts/1")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "2")
                .header("x-role", "admin")
                .header("x-email", "other@example.com")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("other read request should complete");
    assert_eq!(other_published_read.status(), StatusCode::OK);

    // Post 4 is never published, so it's the genuine "non-owner can't
    // read another user's draft over the route" check.
    let other_draft_read = router
        .clone()
        .oneshot(
            Request::get("/advanced_posts/4")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "2")
                .header("x-role", "admin")
                .header("x-email", "other@example.com")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("other draft read request should complete");
    assert_eq!(other_draft_read.status(), StatusCode::NOT_FOUND);

    let blocked_author_read = router
        .clone()
        .oneshot(
            Request::get("/advanced_posts/3")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .header("x-role", "admin")
                .header("x-email", "owner@example.com")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("blocked author read request should complete");
    assert_eq!(blocked_author_read.status(), StatusCode::NOT_FOUND);
}
