//! `cratestack contract ops`: every op a schema exposes with the roles its
//! `@allow` rules name and whether it can be called with no principal
//! (cratestack#1388). Read-only.
//!
//! The analysis is `cratestack_policy::analyse_op_policy`, the parser the
//! macros lower `@allow` from, not a port of it: see that module for exactly
//! what `requires_principal` and `roles` mean, including the no-`@allow`
//! case (denied for everyone, reported as `requires_principal: true` with an
//! empty `allow`) and the fail-closed `unanalysed` list. Keys and digests are
//! the ones `contract digest` prints.
//!
//! The JSON document (`--json`), whose field set only ever grows:
//!
//! ```text
//! { "ok": true, "schema": "<file stem>", "transport": "rest" | "rpc",
//!   "ops": [ { "key", "kind", "allow": [..], "deny": [..], "roles": [..],
//!              "requires_principal", "unanalysed": [..], "contract_digest" } ] }
//! ```
//!
//! `kind` is `procedure`, `subscription`, or `model.<verb>`. An unreadable
//! schema exits 2; with `--json` it prints `{ "ok": false, "error": … }`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli_support::parse_schema_or_render;
use cratestack_core::{Schema, TransportStyle, digest_hex, op_contract_digests};

mod collect;
mod table;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_digest_table;

pub(crate) use collect::OpRow;

/// The report for `schema`, named after `path`'s file stem.
pub(super) fn ops_report(schema: &Schema, path: &Path, json: bool) -> Result<String> {
    let digests = op_contract_digests(schema);
    let rows = collect::rows(schema, |key| {
        digests
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, digest)| digest_hex(digest))
            .unwrap_or_default()
    });
    if !json {
        return Ok(table::render(&rows));
    }
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let transport = match schema.transport {
        TransportStyle::Rpc => "rpc",
        TransportStyle::Rest => "rest",
    };
    let doc =
        serde_json::json!({ "ok": true, "schema": name, "transport": transport, "ops": rows });
    Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?))
}

/// Print the report; an unreadable schema exits 2, and with `--json` says so
/// as `{ "ok": false, "error": … }` on stdout.
pub(super) fn run(path: &PathBuf, json: bool) -> Result<()> {
    let report = parse_schema_or_render(path).and_then(|schema| ops_report(&schema, path, json));
    match report {
        Ok(text) => print!("{text}"),
        Err(error) if json => {
            print!("{}", json_error(&error));
            std::process::exit(2)
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

/// `--json` for a schema that could not be read: the same `ok: false` and
/// `error` shape `contract check` prints.
pub(super) fn json_error(error: &anyhow::Error) -> String {
    format!(
        "{:#}\n",
        serde_json::json!({ "ok": false, "error": format!("{error:#}") })
    )
}
