use std::io::Write;

use super::*;

/// `studio.toml` + a tiny schema, but no DB connection — we drive a
/// load failure on the pool step to prove the rest of the chain is
/// wired. Pure-success paths require a real Postgres and are
/// covered by the testcontainers-gated integration test.
#[tokio::test]
async fn load_reports_pool_failure_with_target_key() {
    let temp = tempfile::tempdir().expect("temp dir");
    let schema_path = temp.path().join("user.cstack");
    let mut schema_file = std::fs::File::create(&schema_path).expect("schema file");
    writeln!(
        schema_file,
        "model User {{\n  id String @id\n  name String\n}}"
    )
    .expect("write schema");

    let config_path = temp.path().join("studio.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
            [workspace]
            name = "smoke"

            [[target]]
            key = "users"
            schema = "{schema}"

            [target.db]
            url = "postgres://nope:nope@127.0.0.1:1/db_does_not_exist"
            driver = "postgres"
            "#,
            schema = schema_path.file_name().unwrap().to_str().unwrap(),
        ),
    )
    .expect("studio.toml writes");

    let error = LoadedWorkspace::load(&config_path)
        .await
        .expect_err("pool connect should fail");
    let message = error.to_string();
    assert!(
        message.contains("target 'users'"),
        "error should name the target, got: {message}"
    );
}

#[tokio::test]
async fn api_only_target_loads_without_a_db() {
    let temp = tempfile::tempdir().expect("temp dir");
    let schema_path = temp.path().join("inv.cstack");
    std::fs::write(
        &schema_path,
        "model Item {\n  id String @id\n  name String\n}",
    )
    .expect("write schema");

    let config_path = temp.path().join("studio.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
            [[target]]
            key = "inv"
            schema = "{schema}"

            [target.api]
            base_url = "https://inventory.internal"
            "#,
            schema = schema_path.file_name().unwrap().to_str().unwrap(),
        ),
    )
    .expect("studio.toml writes");

    let workspace = LoadedWorkspace::load(&config_path)
        .await
        .expect("api-only load succeeds");
    assert_eq!(workspace.targets.len(), 1);
    assert_eq!(workspace.targets[0].key, "inv");
}
