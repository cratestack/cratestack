//! Service-bootstrap batteries for CrateStack applications.
//!
//! Why this crate exists: a schema-first facade (`cratestack-pg`,
//! `cratestack-api`, `cratestack-sqlite`) answers "how do I define and serve
//! my API". It deliberately does not answer "how does my binary's `main()`
//! read its port from the environment", "what does `kubectl` probe to know
//! this pod is ready", or "how do I get a `tracing` subscriber installed
//! before the first log line" — those are the same handful of lines in
//! every service, they have nothing to do with the schema, and a facade
//! that grew them would stop being a facade (`docs/design/layering.md` §2:
//! "a facade that grows a function has stopped being a facade"). This
//! crate is where that handful of lines lives instead, so it is written
//! once rather than once per service.
//!
//! Three pieces, usable independently:
//!
//! - [`telemetry::init`] — a `tracing_subscriber` bootstrap: env-driven
//!   filter, optional JSON output, idempotent (`try_init`).
//! - [`ServiceConfig`]/[`ServiceState`] plus [`health`] — an env-driven
//!   config struct and a `/healthz` + `/healthz/ready` router. Readiness
//!   checks are opt-in: a dependency is only probed when its URL is
//!   configured, so a service with no Redis and no object storage gets a
//!   readiness check that only ever asks Postgres.
//! - [`run`] — installs a request-tracing layer and serves a router.
//!
//! Every environment variable this crate reads is prefixed with a
//! caller-supplied string (`ServiceConfig::from_env("AUTH", ...)` reads
//! `AUTH_SERVICE_HOST`, `AUTH_DATABASE_URL`, ...) — this crate ships no
//! fixed prefix and no per-service defaults (connection strings, database
//! names). Picking those is the caller's job; baking one service's
//! conventions into framework surface is exactly the mistake this crate's
//! own absorption ticket was written to avoid.
//!
//! ## The `postgres` feature (on by default)
//!
//! `ServiceConfig::database_url`/[`ServiceConfig::state`], the readiness
//! Postgres check, and the [`migrations`] module all need
//! `cratestack-sqlx`. They sit behind the `postgres` Cargo feature so that
//! a `cratestack-api` or `cratestack-sqlite` consumer — whose whole point
//! is having no `sqlx` in their dependency graph — can still pull in
//! `telemetry::init`, the rest of `ServiceConfig`, the Redis/object-storage
//! readiness checks, and [`run`] with `default-features = false` and no
//! database binding along for the ride. Mirrors `cratestack-pg`'s own
//! `postgres` feature (same name, same "forwards `dep:cratestack-sqlx`"
//! shape).

pub mod health;
pub mod telemetry;

mod config;
mod run;

#[cfg(feature = "postgres")]
pub mod migrations;

pub use config::ServiceConfig;
pub use run::run;

/// Request-scoped state handed to every `health` handler.
///
/// Cheap to clone (an owned [`ServiceConfig`] plus, under the `postgres`
/// feature, a pooled [`cratestack_sqlx::sqlx::PgPool`] — pools are
/// themselves `Clone` and share their underlying connection set).
#[derive(Clone, Debug)]
pub struct ServiceState {
    pub config: ServiceConfig,
    #[cfg(feature = "postgres")]
    pub pool: cratestack_sqlx::sqlx::PgPool,
}
