//! `/healthz` + `/healthz/ready`.

use std::time::Duration;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;
use tokio::{net::TcpStream, time::timeout};
use url::Url;

use crate::ServiceState;

#[derive(Clone, Debug, Serialize)]
pub struct HealthData {
    pub service: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DependencyState {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadinessData {
    pub service: String,
    pub status: String,
    pub checks: Vec<DependencyState>,
}

/// `/healthz` + `/healthz/ready`, mountable into a larger router via
/// `.merge(cratestack_service::health::router())`.
///
/// Deliberately carries no `.fallback()` — axum panics at request time if
/// two merged routers each set one, and this crate has no way to know
/// whether the router it ends up merged into wants its own 404 handling.
/// The upstream code this was absorbed from did set a fallback, which only
/// ever worked because it happened to be the *last* router merged in every
/// service that used it; that's a footgun for a reusable crate, not a
/// feature, so it was dropped rather than carried forward.
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/healthz/ready", get(readiness))
}

/// Always 200 — liveness answers "is the process up", not "is it useful
/// yet" (that's [`readiness`]). A liveness check that can fail on a
/// dependency invites Kubernetes to restart a pod that a database blip
/// didn't actually break.
pub async fn liveness(State(state): State<ServiceState>) -> impl IntoResponse {
    Json(HealthData {
        service: state.config.service_name.clone(),
        status: "ok".to_string(),
    })
}

/// Checks every *configured* dependency: Postgres unconditionally when the
/// `postgres` feature is enabled, Redis and object storage only when their
/// URL is set on [`ServiceConfig`](crate::ServiceConfig). A service with
/// neither configured gets a readiness check that only ever asks Postgres
/// (or, with `postgres` disabled, one that always passes trivially) —
/// nothing here assumes Redis or object storage exist.
///
/// Returns 200 with `status: "ok"` when every configured check passes, or
/// 503 with `status: "degraded"` and the per-dependency detail otherwise —
/// a real HTTP status, not just a body field, because a kubelet readiness
/// probe only ever looks at the status code.
pub async fn readiness(State(state): State<ServiceState>) -> impl IntoResponse {
    let mut checks = Vec::new();

    #[cfg(feature = "postgres")]
    checks.push(check_postgres(&state.pool).await);

    if let Some(redis_url) = &state.config.redis_url {
        checks.push(check_redis(redis_url).await);
    }

    if let Some(object_storage_endpoint) = &state.config.object_storage_endpoint {
        checks.push(check_object_storage(object_storage_endpoint).await);
    }

    let overall_ok = checks.iter().all(|check| check.status == "ok");
    let status_code = if overall_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(ReadinessData {
            service: state.config.service_name.clone(),
            status: if overall_ok { "ok" } else { "degraded" }.to_string(),
            checks,
        }),
    )
}

#[cfg(feature = "postgres")]
async fn check_postgres(pool: &cratestack_sqlx::sqlx::PgPool) -> DependencyState {
    match timeout(Duration::from_secs(2), pool.acquire()).await {
        Ok(Ok(_connection)) => DependencyState {
            name: "postgres".to_string(),
            status: "ok".to_string(),
            detail: "acquired pooled connection".to_string(),
        },
        Ok(Err(error)) => DependencyState {
            name: "postgres".to_string(),
            status: "error".to_string(),
            detail: error.to_string(),
        },
        Err(_elapsed) => DependencyState {
            name: "postgres".to_string(),
            status: "error".to_string(),
            detail: "timeout while acquiring connection".to_string(),
        },
    }
}

async fn check_redis(target_url: &str) -> DependencyState {
    tcp_check("redis", target_url).await
}

async fn check_object_storage(target_url: &str) -> DependencyState {
    tcp_check("object_storage", target_url).await
}

/// Shared TCP-reachability probe for `redis://`/`http(s)://`-shaped URLs.
/// Neither Redis nor object storage gets a protocol-aware check (no
/// `PING`, no signed `HEAD` request) — a plain TCP connect is enough to
/// catch "the host is unreachable" / "nothing is listening", which is the
/// failure mode a readiness probe exists to catch. A deeper check is each
/// service's own business, not this crate's.
async fn tcp_check(name: &'static str, target_url: &str) -> DependencyState {
    match socket_target(target_url) {
        Ok(target) => {
            match timeout(Duration::from_secs(1), TcpStream::connect(target.as_str())).await {
                Ok(Ok(_stream)) => DependencyState {
                    name: name.to_string(),
                    status: "ok".to_string(),
                    detail: format!("connected to {target}"),
                },
                Ok(Err(error)) => DependencyState {
                    name: name.to_string(),
                    status: "error".to_string(),
                    detail: error.to_string(),
                },
                Err(_elapsed) => DependencyState {
                    name: name.to_string(),
                    status: "error".to_string(),
                    detail: format!("timeout while connecting to {target}"),
                },
            }
        }
        Err(detail) => DependencyState {
            name: name.to_string(),
            status: "error".to_string(),
            detail,
        },
    }
}

fn socket_target(target_url: &str) -> Result<String, String> {
    let parsed = Url::parse(target_url).map_err(|error| error.to_string())?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("missing host in {target_url}"))?;
    let port = parsed
        .port()
        .or_else(|| default_port_for_scheme(parsed.scheme()))
        .ok_or_else(|| format!("missing port in {target_url}"))?;

    Ok(format!("{host}:{port}"))
}

fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    match scheme {
        "postgres" | "postgresql" => Some(5432),
        "redis" => Some(6379),
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{check_object_storage, check_redis, default_port_for_scheme, socket_target};

    #[test]
    fn uses_default_redis_port() {
        assert_eq!(
            socket_target("redis://cache.internal"),
            Ok("cache.internal:6379".to_string())
        );
    }

    #[test]
    fn keeps_an_explicit_port() {
        assert_eq!(
            socket_target("redis://cache.internal:7000"),
            Ok("cache.internal:7000".to_string())
        );
    }

    #[test]
    fn rejects_a_url_with_no_host() {
        assert!(socket_target("redis:///no-host").is_err());
    }

    #[test]
    fn knows_default_ports() {
        assert_eq!(default_port_for_scheme("redis"), Some(6379));
        assert_eq!(default_port_for_scheme("https"), Some(443));
        assert_eq!(default_port_for_scheme("ftp"), None);
    }

    // Port 1 is a reserved, never-listened-on TCP port on every CI runner
    // this test will ever execute on, so `connect` fails immediately with
    // "connection refused" instead of hanging out to the 1s timeout — the
    // opt-in readiness check degrades correctly when the configured
    // dependency isn't actually reachable.
    #[tokio::test]
    async fn check_redis_reports_error_for_an_unreachable_target() {
        let state = check_redis("redis://127.0.0.1:1").await;
        assert_eq!(state.name, "redis");
        assert_eq!(state.status, "error");
    }

    #[tokio::test]
    async fn check_object_storage_reports_error_for_an_unreachable_target() {
        let state = check_object_storage("http://127.0.0.1:1").await;
        assert_eq!(state.name, "object_storage");
        assert_eq!(state.status, "error");
    }

    // End-to-end through the real router: proves readiness only ever
    // checks *configured* dependencies (§ design question 3 — "health
    // check dependencies opt-in rather than assuming Redis and object
    // storage exist"). Neither REDIS_URL nor OBJECT_STORAGE_ENDPOINT is
    // set here, so the only check that runs is Postgres — and it degrades
    // the whole response to 503, because port 1 is never listening.
    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn readiness_skips_unconfigured_optional_dependencies() {
        use axum::body::{Body, to_bytes};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        use crate::ServiceConfig;

        let config = ServiceConfig::from_env_with("TEST", "test-service", 8080, |name: &str| {
            if name == "TEST_DATABASE_URL" {
                Ok("postgres://user:pass@127.0.0.1:1/db".to_string())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .expect("TEST_DATABASE_URL was supplied by the lookup closure above");

        // `connect_lazy_with` never dials the database, so building the
        // state here doesn't touch the network — only the readiness
        // request below does.
        let state = config
            .state()
            .await
            .expect("lazily-connecting pool construction cannot fail on a well-formed URL");

        let response = super::router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/healthz/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let checks = json["checks"].as_array().expect("checks is an array");
        assert_eq!(
            checks.len(),
            1,
            "redis/object storage aren't configured — only postgres should be checked"
        );
        assert_eq!(checks[0]["name"], "postgres");
        assert_eq!(checks[0]["status"], "error");
    }
}
