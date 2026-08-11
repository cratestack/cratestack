use super::*;

#[test]
fn parses_empty_config() {
    let cfg = StudioConfig::parse("").expect("empty config is valid");
    assert_eq!(cfg.workspace.name, "studio");
    assert_eq!(cfg.workspace.default_mode, TargetMode::Ro);
    assert!(cfg.targets.is_empty());
}

#[test]
fn parses_workspace_header() {
    let cfg = StudioConfig::parse(
        r#"
            [workspace]
            name = "acme"
            default_mode = "rw"
        "#,
    )
    .expect("workspace header should parse");
    assert_eq!(cfg.workspace.name, "acme");
    assert_eq!(cfg.workspace.default_mode, TargetMode::Rw);
}

#[test]
fn parses_db_target() {
    let cfg = StudioConfig::parse(
        r#"
            [[target]]
            key = "catalog"
            schema = "schemas/catalog.cstack"

            [target.db]
            url = "env:CATALOG_DB_URL"
            driver = "postgres"
            max_connections = 5
        "#,
    )
    .expect("db target should parse");
    let target = &cfg.targets[0];
    assert_eq!(target.key, "catalog");
    let db = target.db.as_ref().expect("db block present");
    assert_eq!(db.driver, DbDriver::Postgres);
    assert_eq!(db.max_connections, Some(5));
    assert_eq!(cfg.target_mode(target), TargetMode::Ro);
    assert!(
        !db.allow_unsafe_writes,
        "allow_unsafe_writes must default to false (cratestack#507)"
    );
}

#[test]
fn parses_db_target_with_allow_unsafe_writes_opt_in() {
    let cfg = StudioConfig::parse(
        r#"
            [[target]]
            key = "catalog"
            schema = "schemas/catalog.cstack"

            [target.db]
            url = "env:CATALOG_DB_URL"
            driver = "postgres"
            allow_unsafe_writes = true
        "#,
    )
    .expect("db target should parse");
    let db = cfg.targets[0].db.as_ref().expect("db block present");
    assert!(db.allow_unsafe_writes);
}

#[test]
fn parses_api_target_with_bearer_auth() {
    let cfg = StudioConfig::parse(
        r#"
            [[target]]
            key = "accounts"
            schema = "schemas/accounts.cstack"
            mode = "ro"

            [target.api]
            base_url = "https://accounts.internal"
            prefer_for = ["procedures"]
            auth = { kind = "bearer", token = "env:ACCOUNTS_TOKEN" }
        "#,
    )
    .expect("api target should parse");
    let api = cfg.targets[0].api.as_ref().expect("api block present");
    assert_eq!(api.base_url, "https://accounts.internal");
    assert_eq!(api.prefer_for, vec!["procedures".to_owned()]);
    match api.auth.as_ref().expect("auth set") {
        ApiAuth::Bearer { token } => assert_eq!(token, "env:ACCOUNTS_TOKEN"),
        other => panic!("expected bearer auth, got {other:?}"),
    }
}

#[test]
fn rejects_target_without_db_or_api() {
    let error = StudioConfig::parse(
        r#"
            [[target]]
            key = "lonely"
            schema = "schemas/lonely.cstack"
        "#,
    )
    .expect_err("orphaned target should fail validation");
    assert!(matches!(
        error,
        StudioConfigError::TargetMissingChannel { ref key } if key == "lonely"
    ));
}

#[test]
fn rejects_duplicate_keys() {
    let error = StudioConfig::parse(
        r#"
            [[target]]
            key = "dup"
            schema = "a.cstack"
            [target.db]
            url = "sqlite://a.db"
            driver = "sqlite"

            [[target]]
            key = "dup"
            schema = "b.cstack"
            [target.db]
            url = "sqlite://b.db"
            driver = "sqlite"
        "#,
    )
    .expect_err("duplicate keys should fail validation");
    assert!(matches!(
        error,
        StudioConfigError::DuplicateKey { ref key } if key == "dup"
    ));
}

#[test]
fn rejects_invalid_key_characters() {
    let error = StudioConfig::parse(
        r#"
            [[target]]
            key = "has spaces"
            schema = "x.cstack"
            [target.db]
            url = "sqlite://x.db"
            driver = "sqlite"
        "#,
    )
    .expect_err("invalid key should fail");
    assert!(matches!(error, StudioConfigError::InvalidKey { .. }));
}
