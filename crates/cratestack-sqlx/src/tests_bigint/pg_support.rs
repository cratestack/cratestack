//! A Postgres for the `BigInt` policy tests, chosen by environment like the
//! other PG-backed suites (`cratestack-pg/tests/support/pg.rs` is the model;
//! keep the decision in step with the copies listed in its `require_db.rs`):
//!
//! 1. `CRATESTACK_TEST_DATABASE_URL`: connect to that server.
//! 2. `CRATESTACK_USE_TESTCONTAINERS`: start an ephemeral `postgres:18-alpine`.
//! 3. Neither: skip, unless `CRATESTACK_REQUIRE_DB` is set, which makes a
//!    missing or unreachable database a panic. A skipped binary prints `ok`,
//!    so CI sets it.

use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use crate::sqlx::PgPool;
use crate::sqlx::postgres::PgPoolOptions;

#[derive(Debug, PartialEq, Eq)]
enum Backend {
    Url,
    TestContainers,
    Skip,
}

/// Pure so the guard can be shown to fail (`require_without_a_backend_panics`).
fn pick_backend(has_url: bool, use_testcontainers: bool, require: bool) -> Backend {
    if has_url {
        Backend::Url
    } else if use_testcontainers {
        Backend::TestContainers
    } else if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is"
        );
    } else {
        Backend::Skip
    }
}

pub(super) struct TestPg {
    pub pool: PgPool,
    _container: Option<ContainerAsync<Postgres>>,
}

pub(super) async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok();
    let backend = pick_backend(
        url.is_some(),
        std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok(),
        require,
    );
    fn need<T>(require: bool, step: &str, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) if require => {
                panic!("CRATESTACK_REQUIRE_DB is set but {step} failed: {error}")
            }
            Err(_) => None,
        }
    }
    let (url, container) = match backend {
        Backend::Skip => return None,
        Backend::Url => (url.expect("has_url implies the variable"), None),
        Backend::TestContainers => {
            let container = need(
                require,
                "starting the Postgres testcontainer (is Docker available?)",
                Postgres::default()
                    .with_tag("18-alpine")
                    .start()
                    .await
                    .map_err(|e| e.to_string()),
            )?;
            let host = need(
                require,
                "resolving the testcontainer host",
                container.get_host().await.map_err(|e| e.to_string()),
            )?;
            let port = need(
                require,
                "resolving the testcontainer port",
                container
                    .get_host_port_ipv4(5432)
                    .await
                    .map_err(|e| e.to_string()),
            )?;
            (
                format!("postgres://postgres:postgres@{host}:{port}/postgres"),
                Some(container),
            )
        }
    };
    let pool = need(
        require,
        "connecting to Postgres",
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .map_err(|e| e.to_string()),
    )?;
    Some(TestPg {
        pool,
        _container: container,
    })
}

#[test]
fn the_backend_decision_follows_the_environment() {
    assert_eq!(pick_backend(true, true, false), Backend::Url);
    assert_eq!(pick_backend(false, true, true), Backend::TestContainers);
    assert_eq!(pick_backend(false, false, false), Backend::Skip);
}

#[test]
#[should_panic(expected = "CRATESTACK_REQUIRE_DB is set but neither")]
fn require_without_a_backend_panics() {
    pick_backend(false, false, true);
}
