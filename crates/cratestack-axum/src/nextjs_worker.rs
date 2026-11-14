//! Next.js `NativeRouteManager` worker handshake helpers.
//!
//! Next.js's multi-language API route architecture (`NativeRouteManager` in
//! [`packages/next/src/server/lib/native-route.external.ts`](https://github.com/vercel/next.js/blob/canary/packages/next/src/server/lib/native-route.external.ts),
//! introduced in [`vercel/next.js#100130`](https://github.com/vercel/next.js/pull/100130))
//! runs native backend route handlers as a persistent child process per project
//! rather than spawning a process per HTTP request:
//!
//! 1. Next.js spawns the native backend binary with `PORT=0` and waits for a
//!    line matching `/^PORT=(\d+)\s*$/m` on the worker's `stdout`.
//! 2. The worker binds an ephemeral loopback port (`127.0.0.1:0`, or `PORT`
//!    when explicitly supplied), writes `PORT=<port>\n` to `stdout`, flushes
//!    `stdout` immediately, and serves HTTP requests proxied by Next.js to
//!    `http://127.0.0.1:<port><pathname><search>`.
//!
//! This module provides zero-dependency helpers so any CrateStack Axum server
//! can act as a Next.js native API worker as well as a standalone server.

use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Environment variable set to `"1"` or `"true"` when running under a Next.js
/// native route worker supervisor.
pub const NEXTJS_WORKER_ENV: &str = "CRATESTACK_NEXTJS_WORKER";

/// Standard `PORT` environment variable checked before falling back to an
/// ephemeral or default port.
pub const PORT_ENV: &str = "PORT";

/// Resolves the socket address a CrateStack HTTP server should bind to given a
/// `fallback` address string (such as `"127.0.0.1:8080"`):
///
/// - If `PORT` is set in the environment and parses as a `u16`, its value
///   overrides the port of `fallback` (including `PORT=0` for an OS-assigned
///   ephemeral port).
/// - Otherwise, if `CRATESTACK_NEXTJS_WORKER` is set to `"1"` or `"true"`, the
///   port is set to `0` on `127.0.0.1` so the OS assigns a collision-free
///   ephemeral loopback port for Next.js `NativeRouteManager`.
/// - Otherwise, `fallback` is parsed and returned as-is.
pub fn resolve_nextjs_worker_addr(fallback: &str) -> io::Result<SocketAddr> {
    let env_port = std::env::var(PORT_ENV).ok();
    let worker_flag = std::env::var(NEXTJS_WORKER_ENV).ok();
    resolve_nextjs_worker_addr_from(fallback, env_port.as_deref(), worker_flag.as_deref())
}

/// Pure, testable counterpart to [`resolve_nextjs_worker_addr`] that takes the
/// optional `PORT` and `CRATESTACK_NEXTJS_WORKER` values explicitly instead of
/// reading process environment variables.
pub fn resolve_nextjs_worker_addr_from(
    fallback: &str,
    env_port: Option<&str>,
    worker_flag: Option<&str>,
) -> io::Result<SocketAddr> {
    let base: SocketAddr = fallback
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    if let Some(raw_port) = env_port.map(str::trim).filter(|s| !s.is_empty()) {
        let port: u16 = raw_port.parse().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid PORT value {raw_port:?}: {e}"),
            )
        })?;
        return Ok(SocketAddr::new(base.ip(), port));
    }

    if is_truthy_flag(worker_flag) {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0));
    }

    Ok(base)
}

/// Formats the exact `PORT=<port>\n` handshake line expected by Next.js's
/// `NativeRouteManager` (`/^PORT=(\d+)/m`).
pub fn format_port_handshake(addr: &SocketAddr) -> String {
    format!("PORT={}\n", addr.port())
}

/// Writes `PORT=<port>\n` to `writer` and flushes it immediately.
pub fn write_port_handshake<W: Write>(writer: &mut W, addr: &SocketAddr) -> io::Result<()> {
    let line = format_port_handshake(addr);
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

/// Emits `PORT=<port>\n` to process `stdout` and flushes immediately so a
/// parent Next.js `NativeRouteManager` completes its startup handshake without
/// buffering delay.
pub fn emit_port_handshake(addr: &SocketAddr) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    write_port_handshake(&mut handle, addr)
}

/// Emits the `PORT=<port>\n` handshake for `listener.local_addr()` to `stdout`
/// and serves `app` on `listener` via [`axum::serve`].
pub async fn serve_nextjs_worker<L>(listener: L, app: axum::Router) -> io::Result<()>
where
    L: axum::serve::Listener<Addr = SocketAddr>,
{
    let local_addr = listener.local_addr()?;
    emit_port_handshake(&local_addr)?;
    axum::serve(listener, app).await
}

fn is_truthy_flag(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some(v) if v == "1" || v.eq_ignore_ascii_case("true")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};

    #[test]
    fn resolve_addr_uses_fallback_when_no_env_set() {
        let addr = resolve_nextjs_worker_addr_from("127.0.0.1:8080", None, None).unwrap();
        assert_eq!(addr, "127.0.0.1:8080".parse().unwrap());
    }

    #[test]
    fn resolve_addr_uses_ephemeral_loopback_port_when_worker_flag_enabled() {
        let addr = resolve_nextjs_worker_addr_from("0.0.0.0:8080", None, Some("1")).unwrap();
        assert_eq!(addr, "127.0.0.1:0".parse().unwrap());

        let addr_true =
            resolve_nextjs_worker_addr_from("127.0.0.1:8080", None, Some("true")).unwrap();
        assert_eq!(addr_true, "127.0.0.1:0".parse().unwrap());
    }

    #[test]
    fn explicit_port_overrides_fallback_and_worker_default() {
        let addr =
            resolve_nextjs_worker_addr_from("127.0.0.1:8080", Some("4321"), Some("1")).unwrap();
        assert_eq!(addr, "127.0.0.1:4321".parse().unwrap());

        let ephemeral = resolve_nextjs_worker_addr_from("127.0.0.1:8080", Some("0"), None).unwrap();
        assert_eq!(ephemeral, "127.0.0.1:0".parse().unwrap());
    }

    #[test]
    fn write_port_handshake_emits_nextjs_native_route_manager_line() {
        let addr: SocketAddr = "127.0.0.1:54321".parse().unwrap();
        let mut buf = Vec::new();
        write_port_handshake(&mut buf, &addr).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "PORT=54321\n");
    }

    #[tokio::test]
    async fn binds_ephemeral_port_and_formats_nonzero_handshake() {
        let bind_addr = resolve_nextjs_worker_addr_from("127.0.0.1:8080", None, Some("1")).unwrap();
        let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        assert_ne!(local_addr.port(), 0);

        let mut handshake = Vec::new();
        write_port_handshake(&mut handshake, &local_addr).unwrap();
        let line = String::from_utf8(handshake).unwrap();
        assert_eq!(line, format!("PORT={}\n", local_addr.port()));

        let app = Router::new().route("/api/health", get(|| async { "ok" }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        server.abort();
    }
}
