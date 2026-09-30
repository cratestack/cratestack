//! `cratestack contract digest|print|lock|check|prune`: the per-op contract
//! digests a signed client binds and the lock of older ones a server keeps
//! accepting (cratestack#1123). `digest` and `print` are read-only; `check`
//! is the CI gate (`lock_cmd`).

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Subcommand;
use cratestack_core::{
    Schema, client_contract_digest, digest_hex, op_contract_digests, op_contract_json,
};

use crate::cli_support::parse_schema_or_render;

mod lock_cmd;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lock;

#[derive(Debug, Subcommand)]
pub(crate) enum ContractAction {
    /// Print every op's contract digest and the client contract digest.
    Digest {
        #[arg(long)]
        schema: PathBuf,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Print the canonical contract JSON one op's digest is taken over:
    /// the tool for "why did this op's digest move".
    Print {
        #[arg(long)]
        schema: PathBuf,
        /// The op key: an RPC `op_id` (`procedure.ping`) or, on REST,
        /// `"<METHOD> <route template>"` (`GET /widgets/{id}`).
        #[arg(long)]
        op: String,
    },
    /// Record the schema's current op contracts in the lock file as a new
    /// generation (creating the file), so a server built with
    /// `contracts = "<lock>"` keeps accepting clients built from it while
    /// they stay wire-compatible. Run it before shipping a client build.
    /// Refuses, leaving the file alone, while the current contract breaks a
    /// locked one: `prune --op` that op first.
    Lock {
        #[arg(long)]
        schema: PathBuf,
        /// The lock file, committed next to the schema.
        #[arg(long)]
        lock: PathBuf,
        /// Which store build or release carries this generation.
        #[arg(long, default_value = "")]
        note: String,
        /// `YYYY-MM-DD`; today (UTC) when omitted.
        #[arg(long)]
        date: Option<String>,
    },
    /// CI gate: exit 1 when the current contract is not a locked generation,
    /// or breaks a locked one (the reasons are printed).
    Check {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        lock: PathBuf,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Drop history from the lock: one op's older clients, old generations,
    /// or all but the newest few. Exactly one selector.
    #[command(group(clap::ArgGroup::new("what").required(true).multiple(false)))]
    Prune {
        #[arg(long)]
        lock: PathBuf,
        /// Stop accepting older clients of this op (how a breaking change
        /// to one op is shipped on purpose).
        #[arg(long, group = "what")]
        op: Option<String>,
        /// Remove generations locked before this `YYYY-MM-DD`.
        #[arg(long, group = "what")]
        before: Option<String>,
        /// Keep only the newest N generations.
        #[arg(long, group = "what")]
        keep: Option<usize>,
        /// Remove the generation whose client contract digest starts with
        /// this (at least 8 hex digits).
        #[arg(long, group = "what")]
        generation: Option<String>,
    },
}

pub(crate) fn run(action: ContractAction) -> Result<()> {
    match action {
        ContractAction::Digest { schema, json } => {
            print!("{}", digest_report(&parse_schema_or_render(&schema)?, json));
            Ok(())
        }
        ContractAction::Print { schema, op } => {
            println!("{}", print_report(&parse_schema_or_render(&schema)?, &op)?);
            Ok(())
        }
        ContractAction::Lock {
            schema,
            lock,
            note,
            date,
        } => {
            let date = date.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
            finish(lock_cmd::lock(
                &parse_schema_or_render(&schema)?,
                &lock,
                &note,
                &date,
            )?)
        }
        ContractAction::Check { schema, lock, json } => finish(lock_cmd::check(
            &parse_schema_or_render(&schema)?,
            &lock,
            json,
        )?),
        ContractAction::Prune {
            lock,
            op,
            before,
            keep,
            generation,
        } => {
            let what = match (op, before, keep, generation) {
                (Some(op), ..) => lock_cmd::Prune::Op(op),
                (_, Some(date), ..) => lock_cmd::Prune::Before(date),
                (_, _, Some(keep), _) => lock_cmd::Prune::Keep(keep),
                (.., Some(prefix)) => lock_cmd::Prune::Generation(prefix),
                _ => bail!("give one of --op, --before, --keep, --generation"),
            };
            finish(lock_cmd::prune(&lock, what)?)
        }
    }
}

/// Print the report; a failed verdict exits 1, like `check` and `diff`.
fn finish(outcome: lock_cmd::Outcome) -> Result<()> {
    print!("{}", outcome.text);
    if !outcome.ok {
        std::process::exit(1);
    }
    Ok(())
}

fn digest_report(schema: &Schema, json: bool) -> String {
    let table = op_contract_digests(schema);
    let client = digest_hex(&client_contract_digest(schema));
    if json {
        let ops: serde_json::Map<String, serde_json::Value> = table
            .iter()
            .map(|(key, digest)| (key.clone(), digest_hex(digest).into()))
            .collect();
        let doc = serde_json::json!({ "client_contract": client, "ops": ops });
        return format!("{}\n", serde_json::to_string_pretty(&doc).expect("JSON"));
    }
    let mut out = format!("client contract  {client}\n");
    for (key, digest) in &table {
        out.push_str(&format!("{}  {key}\n", digest_hex(digest)));
    }
    out
}

fn print_report(schema: &Schema, op: &str) -> Result<String> {
    match op_contract_json(schema, op) {
        Some(json) => Ok(json),
        None => {
            let known: Vec<String> = op_contract_digests(schema)
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            bail!("no op `{op}` in this schema; ops: {}", known.join(", "))
        }
    }
}
