//! Input checks for [`super::PgPoolConfig`]. Messages never echo the URL.

use std::str::FromStr;

use cratestack_core::CratestackError;

use crate::sqlx::postgres::PgConnectOptions;

fn bad(message: &str) -> CratestackError {
    CratestackError::BadRequest(message.to_owned())
}

/// Parse a `postgres://` URL, refusing a key=value DSN with a clear message.
pub(super) fn parse_url(url: &str) -> Result<PgConnectOptions, CratestackError> {
    let url = url.trim();
    if !url.contains("://") {
        return Err(if url.contains('=') {
            bad(
                "database connection string is a libpq key=value DSN, which is not supported; \
                 use a postgres://user:password@host:port/database URL",
            )
        } else {
            bad("database connection string is not a postgres:// or postgresql:// URL")
        });
    }
    let scheme_ok = ["postgres://", "postgresql://"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    });
    if !scheme_ok {
        return Err(bad(
            "database connection string is not a postgres:// or postgresql:// URL",
        ));
    }
    PgConnectOptions::from_str(url).map_err(|_| {
        bad("database URL is invalid; expected \
             postgres://user:password@host:port/database (check scheme, host and port)")
    })
}

/// Names go into a startup `-c name=value` string, so they are held to the
/// shape of a Postgres setting name; values only have to be one clean line
/// (sqlx escapes the space and backslash).
pub(super) fn check_setting(name: &str, value: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let first_ok = matches!(chars.next(), Some('a'..='z' | '_'));
    let rest_ok = chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.'));
    if !(first_ok && rest_ok) {
        return Err("session setting name must match [a-z_][a-z0-9_.]*".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "session setting `{name}` has a control character in its value"
        ));
    }
    Ok(())
}
