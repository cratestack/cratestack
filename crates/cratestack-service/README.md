# cratestack-service

Service-bootstrap batteries for CrateStack applications: the handful of
lines every service's `main()` needs that have nothing to do with a
schema — env-driven config, `/healthz` + `/healthz/ready`, a
`tracing_subscriber` initializer, and a `run()` helper.

## Overview

A schema-first facade (`cratestack-pg`, `cratestack-api`,
`cratestack-sqlite`) answers "how do I define and serve my API." It
deliberately does not answer "how does my binary read its port from the
environment," "what does `kubectl` probe to know this pod is ready," or
"how do I get a `tracing` subscriber installed before the first log
line." `cratestack-service` is where that lives instead.

- [`telemetry::init`] — `tracing_subscriber` bootstrap: env-driven filter,
  optional JSON output, idempotent.
- [`ServiceConfig`] + [`health`] — env-driven config and a
  `/healthz`/`/healthz/ready` router. Every readiness check is opt-in:
  Redis and object storage are only probed when their URL is configured.
- [`run`] — installs request tracing and serves a router.
- [`migrations`] (feature `postgres`, default on) — load an
  `include_dir!`-embedded migration tree and apply it.

Every environment variable is prefixed with a caller-supplied string —
`ServiceConfig::from_env("AUTH", "auth-service", 8080)` reads
`AUTH_SERVICE_HOST`, `AUTH_DATABASE_URL`, etc. This crate ships no fixed
prefix, no default connection string, and no service-name-to-database-name
table: those are application specifics, not framework surface.

## Installation

```toml
[dependencies]
cratestack-service = "0.7"
```

A `cratestack-api`/`cratestack-sqlite` service that wants the config/
health/telemetry/run surface without pulling in `sqlx`:

```toml
[dependencies]
cratestack-service = { version = "0.7", default-features = false }
```

With `postgres` disabled, `ServiceConfig` has no `database_url` field, the
readiness check never probes Postgres, and the [`migrations`] module does
not exist — `cargo tree` shows no `cratestack-sqlx` (and therefore no
`sqlx`) anywhere in the graph, matching the guarantee `cratestack-api`
itself makes.

## Usage

```rust,no_run
use cratestack_service::{ServiceConfig, health, run, telemetry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    telemetry::init("AUTH");

    let config = ServiceConfig::from_env("AUTH", "auth-service", 8080)?;
    let state = config.state().await?;

    let app = health::router() // /healthz, /healthz/ready
        // .merge(your_schema_router)
        .with_state(state);

    run(app, &config).await?;
    Ok(())
}
```

Applying embedded migrations before serving:

```rust,ignore
use cratestack_service::migrations::{migrations_from_dir, run_migrations};
use include_dir::{Dir, include_dir};

static MIGRATIONS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations/postgres");

run_migrations(&config.database_url, &migrations_from_dir(&MIGRATIONS)).await?;
```

## See Also

- `cratestack-sqlx` — `Migration`, `apply_pending`, and everything else
  this crate's `migrations` module builds on.
- `cratestack-pg`/`cratestack-api`/`cratestack-sqlite` — pick one for the
  schema-first API surface; `cratestack-service` is a sibling, not a
  replacement.

## License

MIT
