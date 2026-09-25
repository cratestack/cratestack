//! A real Redis for `tests/auth_redis_nonce.rs`, chosen the way
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
}
