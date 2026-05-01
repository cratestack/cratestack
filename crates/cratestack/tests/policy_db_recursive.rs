use cratestack::include_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CoolContext, CoolError, Value};

include_schema!("tests/fixtures/recursive_policy.cstack");

#[tokio::test]
async fn db_backed_recursive_relation_policies_cover_quantifiers_and_create_checks() {
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

    cratestack::sqlx::query(
        "DROP TABLE IF EXISTS tasks, memberships, projects, users, organizations",
    )
    .execute(&pool)
    .await
    .expect("recursive policy tables should reset");
    cratestack::sqlx::query(
        "CREATE TABLE organizations (id BIGINT PRIMARY KEY, slug TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("organizations table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL, banned BOOLEAN NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("users table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE projects (id BIGINT PRIMARY KEY, name TEXT NOT NULL, organization_id BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("projects table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE memberships (id BIGINT PRIMARY KEY, project_id BIGINT NOT NULL, user_id BIGINT NOT NULL, active BOOLEAN NOT NULL, blocked BOOLEAN NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("memberships table should exist");
    cratestack::sqlx::query(
        "CREATE TABLE tasks (id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY, title TEXT NOT NULL, project_id BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("tasks table should exist");

    cratestack::sqlx::query("INSERT INTO organizations (id, slug) VALUES (1, 'alpha'), (2, 'beta')")
        .execute(&pool)
        .await
        .expect("organizations should seed");
    cratestack::sqlx::query(
        "INSERT INTO users (id, email, banned) VALUES (1, 'owner@example.com', FALSE), (2, 'other@example.com', FALSE), (3, 'banned@example.com', TRUE)",
    )
    .execute(&pool)
    .await
    .expect("users should seed");
    cratestack::sqlx::query(
        "INSERT INTO projects (id, name, organization_id) VALUES (1, 'Alpha Good', 1), (2, 'Alpha Banned Member', 1), (3, 'Beta Other', 2), (4, 'Alpha Blocked', 1), (5, 'Alpha Inactive', 1)",
    )
    .execute(&pool)
    .await
    .expect("projects should seed");
    cratestack::sqlx::query(
        "INSERT INTO memberships (id, project_id, user_id, active, blocked) VALUES (1, 1, 1, TRUE, FALSE), (2, 1, 2, TRUE, FALSE), (3, 2, 1, TRUE, FALSE), (4, 2, 3, TRUE, FALSE), (5, 3, 2, TRUE, FALSE), (6, 4, 1, TRUE, TRUE), (7, 5, 1, FALSE, FALSE)",
    )
    .execute(&pool)
    .await
    .expect("memberships should seed");
    cratestack::sqlx::query(
        "INSERT INTO tasks (id, title, project_id) VALUES (1, 'Visible task', 1), (2, 'Banned task', 2), (3, 'Other org task', 3), (4, 'Inactive members task', 5)",
    )
    .execute(&pool)
    .await
    .expect("tasks should seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let owner = CoolContext::authenticated([
        ("id".to_owned(), Value::Int(1)),
        (
            "email".to_owned(),
            Value::String("owner@example.com".to_owned()),
        ),
        ("orgSlug".to_owned(), Value::String("alpha".to_owned())),
    ]);
    let other = CoolContext::authenticated([
        ("id".to_owned(), Value::Int(2)),
        (
            "email".to_owned(),
            Value::String("other@example.com".to_owned()),
        ),
        ("orgSlug".to_owned(), Value::String("beta".to_owned())),
    ]);

    let visible = cool
        .task()
        .find_unique(1_i64)
        .run(&owner)
        .await
        .expect("recursive read should scope cleanly")
        .expect("owner should see task through nested relation policy");
    assert_eq!(visible.id, 1);

    let hidden_by_deny = cool
        .task()
        .find_unique(2_i64)
        .run(&owner)
        .await
        .expect("deny-scoped read should succeed");
    assert!(hidden_by_deny.is_none());

    let hidden_by_membership = cool
        .task()
        .find_unique(1_i64)
        .run(&other)
        .await
        .expect("cross-org read should succeed");
    assert!(hidden_by_membership.is_none());

    let updated = cool
        .task()
        .update(1_i64)
        .set(cratestack_schema::UpdateTaskInput {
            title: Some("Updated visible task".to_owned()),
            projectId: None,
        })
        .run(&owner)
        .await
        .expect("nested relation update should succeed");
    assert_eq!(updated.title, "Updated visible task");

    let denied_update = cool
        .task()
        .update(1_i64)
        .set(cratestack_schema::UpdateTaskInput {
            title: Some("Blocked update".to_owned()),
            projectId: None,
        })
        .run(&other)
        .await
        .expect_err("cross-org update should fail");
    assert!(matches!(denied_update, CoolError::Forbidden(_)));

    let denied_delete = cool
        .task()
        .delete(4_i64)
        .run(&owner)
        .await
        .expect_err("every quantifier should block delete when a member is inactive");
    assert!(matches!(denied_delete, CoolError::Forbidden(_)));

    let deleted = cool
        .task()
        .delete(1_i64)
        .run(&owner)
        .await
        .expect("every quantifier should allow delete when all members are active");
    assert_eq!(deleted.id, 1);

    let created = cool
        .task()
        .create(cratestack_schema::CreateTaskInput {
            title: "Allowed task".to_owned(),
            projectId: 1,
        })
        .run(&owner)
        .await
        .expect("create should honor nested relation checks through SQL");
    assert_eq!(created.projectId, 1);

    let blocked_create = cool
        .task()
        .create(cratestack_schema::CreateTaskInput {
            title: "Blocked by none".to_owned(),
            projectId: 4,
        })
        .run(&owner)
        .await
        .expect_err("blocked membership should deny create");
    assert!(matches!(blocked_create, CoolError::Forbidden(_)));

    let wrong_org_create = cool
        .task()
        .create(cratestack_schema::CreateTaskInput {
            title: "Wrong org".to_owned(),
            projectId: 3,
        })
        .run(&owner)
        .await
        .expect_err("org mismatch should deny create");
    assert!(matches!(wrong_org_create, CoolError::Forbidden(_)));
}
