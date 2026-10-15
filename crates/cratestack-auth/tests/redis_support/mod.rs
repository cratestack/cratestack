//! A real Redis for the `tests/redis_nonce_*.rs` suites, chosen the way
//! `cratestack-redis`'s suites choose theirs
//! (`crates/cratestack-redis/tests/support/redis.rs`):
//!
//! 1. `CRATESTACK_REDIS_TEST_URL`: an external Redis at that URL;
//! 2. `CRATESTACK_USE_TESTCONTAINERS=1`: an ephemeral container, removed
//!    when the returned guard drops;
//! 3. neither: `None`, and the test skips.
//!
//! `CRATESTACK_REQUIRE_REDIS` turns every skip and every connection
//! failure into a panic, so a CI job (or a rootless-Docker host whose
//! `DOCKER_HOST` is unset) cannot pass by skipping.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use chrono::{Duration, Utc};
use cratestack_auth::{AuthError, NonceStore};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::Redis;

pub struct TestRedis {
    pub url: String,
    _container: Option<ContainerAsync<Redis>>,
}

fn need<T, E: std::fmt::Display>(result: Result<T, E>, require: bool, what: &str) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if require => {
            panic!("CRATESTACK_REQUIRE_REDIS is set but {what} failed: {error}")
        }
        Err(_) => None,
    }
}

pub async fn connect_or_skip() -> Option<TestRedis> {
    let require = std::env::var("CRATESTACK_REQUIRE_REDIS").is_ok();
    let (url, container) = if let Ok(url) = std::env::var("CRATESTACK_REDIS_TEST_URL") {
        (url, None)
    } else if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        // The tag `cratestack-redis` pins (its default, 5.0, is EOL).
        let container = need(
            Redis::default().with_tag("7.4").start().await,
            require,
            "starting the Redis testcontainer (is DOCKER_HOST right?)",
        )?;
        let host = need(container.get_host().await, require, "resolving its host")?;
        let port = need(
            container.get_host_port_ipv4(6379).await,
            require,
            "resolving its port",
        )?;
        (format!("redis://{host}:{port}"), Some(container))
    } else if require {
        panic!(
            "CRATESTACK_REQUIRE_REDIS is set but neither CRATESTACK_REDIS_TEST_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is: the Redis suite would skip silently"
        );
    } else {
        return None;
    };
    let client = need(
        redis::Client::open(url.as_str()),
        require,
        "parsing the URL",
    )?;
    need(client.get_connection(), require, "connecting")?;
    Some(TestRedis {
        url,
        _container: container,
    })
}

impl TestRedis {
    pub async fn connection(&self) -> redis::aio::MultiplexedConnection {
        redis::Client::open(self.url.as_str())
            .expect("url")
            .get_multiplexed_async_connection()
            .await
            .expect("connect")
    }

    /// True when this test owns the server (an ephemeral container), or
    /// when `CRATESTACK_REDIS_TEST_ALLOW_RECONFIGURE` says the external one
    /// may be reconfigured (`CONFIG SET requirepass`) and is restored after.
    pub fn may_reconfigure(&self) -> bool {
        self._container.is_some()
            || std::env::var("CRATESTACK_REDIS_TEST_ALLOW_RECONFIGURE").is_ok()
    }
}

/// Tests touch server-wide state (the connection counter, `CLIENT KILL`,
/// `CLIENT PAUSE`, passwords), so they take this and can share one
/// `CRATESTACK_REDIS_TEST_URL` under the default parallel runner.
pub static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `total_connections_received` from `INFO stats`: every TCP connection the
/// server has accepted since it started.
pub async fn connections_received(conn: &mut redis::aio::MultiplexedConnection) -> u64 {
    let info: String = redis::cmd("INFO")
        .arg("stats")
        .query_async(conn)
        .await
        .expect("INFO stats");
    info.lines()
        .find_map(|line| line.strip_prefix("total_connections_received:"))
        .expect("total_connections_received in INFO stats")
        .trim()
        .parse()
        .expect("numeric connection counter")
}

pub async fn claim(store: &dyn NonceStore, nonce: &str) -> Result<(), AuthError> {
    store
        .claim("vk_1070", nonce, Utc::now(), Duration::seconds(300))
        .await
}

/// Distinguishes this run's nonces from an earlier run's keys still alive
/// (TTL 300 s) on a shared server.
pub fn run_tag() -> String {
    Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_default()
        .to_string()
}
