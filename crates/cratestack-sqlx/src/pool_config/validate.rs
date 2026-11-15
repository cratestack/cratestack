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
    let options = PgConnectOptions::from_str(url).map_err(|_| {
        bad("database URL is invalid; expected \
             postgres://user:password@host:port/database (check scheme, host and port)")
    })?;
    // Session settings are appended after the URL's `options`; an unpaired
    // trailing backslash (an odd run of them) would escape the separating
    // space and merge the two. An even run is escaped backslashes: fine.
    let trailing = options
        .get_options()
        .map_or(0, |o| o.chars().rev().take_while(|&c| c == '\\').count());
    if trailing % 2 == 1 {
        return Err(bad(
            "database URL `options` value must not end in an unpaired backslash",
        ));
    }
    if let Some(name) = options.get_options().and_then(sqlx_owned_in_options) {
        return Err(bad(&format!(
            "database URL `options` sets `{name}`, which sqlx sets itself at startup, after any \
             -c option, so the URL value would be silently ignored"
        )));
    }
    Ok(options)
}

/// The first sqlx-owned name a URL `options` string tries to set, through
/// `-c name=v`, `-cname=v` or `--name=v`.
fn sqlx_owned_in_options(options: &str) -> Option<&'static str> {
    let mut after_c = false;
    for token in options.split_whitespace() {
        let assignment = if after_c {
            Some(token)
        } else if token == "-c" {
            None
        } else {
            token
                .strip_prefix("--")
                .or_else(|| token.strip_prefix("-c"))
        };
        after_c = token == "-c";
        let Some(assignment) = assignment else {
            continue;
        };
        let name = assignment.split('=').next().unwrap_or_default();
        if let Some(owned) = SQLX_OWNED
            .iter()
            .find(|owned| name.eq_ignore_ascii_case(owned))
        {
            return Some(owned);
        }
    }
    None
}

/// Settings sqlx sends itself as startup parameters, which the server applies
/// after `-c` options, so a `-c` for them would lose.
const SQLX_OWNED: [&str; 5] = [
    "datestyle",
    "client_encoding",
    "timezone",
    "extra_float_digits",
    "application_name",
];

/// Names go into a startup `-c name=value` string, so they are held to the
/// shape of a Postgres setting name; values only have to be one clean line
/// (sqlx escapes the space and backslash).
pub(super) fn check_setting(name: &str, value: &str) -> Result<(), String> {
    if SQLX_OWNED
        .iter()
        .any(|owned| name.eq_ignore_ascii_case(owned))
    {
        let hint = if name.eq_ignore_ascii_case("application_name") {
            "; use PgPoolConfig::application_name"
        } else {
            ""
        };
        return Err(format!(
            "session setting `{name}` cannot be set: sqlx sets it at startup, after any -c \
             option{hint}"
        ));
    }
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
