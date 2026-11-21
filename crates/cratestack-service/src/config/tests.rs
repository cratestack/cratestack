use std::collections::HashMap;

use super::ServiceConfig;

fn lookup_from(
    vars: &'static [(&'static str, &'static str)],
) -> impl Fn(&str) -> Result<String, std::env::VarError> {
    let map: HashMap<&'static str, &'static str> = vars.iter().copied().collect();
    move |name: &str| {
        map.get(name)
            .map(|value| value.to_string())
            .ok_or(std::env::VarError::NotPresent)
    }
}

#[test]
fn env_prefix_is_honoured() {
    let lookup = lookup_from(&[
        ("AUTH_SERVICE_HOST", "10.0.0.5"),
        ("AUTH_SERVICE_PORT", "9090"),
        ("AUTH_DATABASE_URL", "postgres://a/b"),
        ("AUTH_PUBLIC_BASE_URL", "https://auth.example.com"),
        ("AUTH_REDIS_URL", "redis://localhost:6379"),
        // A same-suffix variable under a DIFFERENT prefix must be
        // ignored — proves the check isn't accidentally matching on
        // the bare suffix.
        ("CATALOG_SERVICE_HOST", "should-not-be-picked-up"),
    ]);

    let config = ServiceConfig::from_env_with("AUTH", "auth-service", 8080, &lookup)
        .expect("all required vars are present");

    assert_eq!(config.host, "10.0.0.5");
    assert_eq!(config.port, 9090);
    assert_eq!(config.public_base_url, "https://auth.example.com");
    assert_eq!(config.redis_url.as_deref(), Some("redis://localhost:6379"));
    #[cfg(feature = "postgres")]
    assert_eq!(config.database_url, "postgres://a/b");
}

#[test]
fn falls_back_to_generic_defaults_when_unset() {
    let lookup = lookup_from(&[("CATALOG_DATABASE_URL", "postgres://c/d")]);

    let config = ServiceConfig::from_env_with("CATALOG", "catalog-service", 8083, &lookup)
        .expect("database url present");

    assert_eq!(config.host, "0.0.0.0");
    assert_eq!(config.port, 8083);
    assert_eq!(config.public_base_url, "http://127.0.0.1:8083");
    assert_eq!(config.redis_url, None);
    assert_eq!(config.object_storage_endpoint, None);
}

#[cfg(feature = "postgres")]
#[test]
fn missing_database_url_is_an_error() {
    let lookup = lookup_from(&[]);

    let error = ServiceConfig::from_env_with("VENDOR", "vendor-service", 8084, &lookup)
        .expect_err("no VENDOR_DATABASE_URL was provided");

    assert!(matches!(
        error,
        cratestack_core::CratestackError::Internal(_)
    ));
    assert!(error.to_string().contains("VENDOR_DATABASE_URL"));
}

#[test]
fn require_redis_url_reports_the_component_and_prefix() {
    // `SEARCH_DATABASE_URL` is present even when the `postgres` feature
    // is off, where it is simply never looked up — harmless either way.
    let lookup = lookup_from(&[("SEARCH_DATABASE_URL", "postgres://s/t")]);
    let config = ServiceConfig::from_env_with("SEARCH", "search-service", 8085, &lookup)
        .expect("required vars present");

    let error = config
        .require_redis_url("rate limiter")
        .expect_err("SEARCH_REDIS_URL was never set");

    let message = error.to_string();
    assert!(message.contains("rate limiter"));
    assert!(message.contains("SEARCH_REDIS_URL"));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn state_builds_a_lazy_pool_of_five_for_a_valid_url() {
    let lookup = lookup_from(&[("APP_DATABASE_URL", "  postgres://u:p@127.0.0.1:1/db  ")]);
    let config = ServiceConfig::from_env_with("APP", "app", 8080, &lookup).unwrap();
    let state = config.state().await.expect("valid url, nothing connects");
    assert_eq!(state.pool.options().get_max_connections(), 5);
    assert_eq!(state.pool.size(), 0, "the pool must be lazy");
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn state_rejects_a_bad_url_without_echoing_it() {
    for url in [
        "mysql://u:hunter2secret@h/db",
        "postgres://u:hunter2secret@h:notaport/db",
    ] {
        let lookup: &'static [(&str, &str)] = Box::leak(Box::new([("APP_DATABASE_URL", url)]));
        let config = ServiceConfig::from_env_with("APP", "app", 8080, lookup_from(lookup)).unwrap();
        let Err(error) = config.state().await else {
            panic!("invalid url must be refused");
        };
        let cratestack_core::CratestackError::Internal(message) = &error else {
            panic!("expected Internal, got {error:?}");
        };
        assert!(message.starts_with("invalid database url"), "{message}");
        assert!(!message.contains("hunter2secret"), "{message}");
    }
}

/// `{:?}` of a config carries no credential: not the password in the
/// database or Redis URL, not the object-storage secret key. Everything else
/// still prints.
#[test]
fn debug_output_redacts_every_secret() {
    let lookup = lookup_from(&[
        (
            "AUTH_DATABASE_URL",
            "postgres://svc:dbs3cret@db.internal:5432/auth",
        ),
        ("AUTH_REDIS_URL", "redis://:rds3cret@cache.internal:6379/0"),
        ("AUTH_OBJECT_STORAGE_ACCESS_KEY", "AKIAPUBLICID"),
        ("AUTH_OBJECT_STORAGE_SECRET_KEY", "ossecret"),
        ("AUTH_OBJECT_STORAGE_BUCKET", "media"),
    ]);
    let config = ServiceConfig::from_env_with("AUTH", "auth-service", 8080, &lookup)
        .expect("all required vars are present");

    let shown = format!("{config:?}");
    for secret in ["dbs3cret", "rds3cret", "ossecret"] {
        assert!(!shown.contains(secret), "{secret} leaked: {shown}");
    }
    assert!(!format!("{config:#?}").contains("dbs3cret"));
    for kept in [
        "auth-service",
        "db.internal",
        "svc",
        "media",
        "AKIAPUBLICID",
    ] {
        assert!(shown.contains(kept), "{kept} missing: {shown}");
    }
}

/// A database URL that does not parse is hidden whole: it could be a
/// key=value DSN with a password in it.
#[test]
fn debug_output_hides_an_unparsable_database_url() {
    let lookup = lookup_from(&[("AUTH_DATABASE_URL", "host=db password=dsns3cret")]);
    let config = ServiceConfig::from_env_with("AUTH", "auth-service", 8080, &lookup)
        .expect("all required vars are present");
    assert!(!format!("{config:?}").contains("dsns3cret"));
}
