//! `cratestack contract lock|check|prune`: the compatible-contract lock
//! (cratestack#1123). `lock` records the schema's current op contracts as a
//! generation, `check` is the CI gate, `prune` drops history. Each returns
//! its report and whether it succeeded; `run` prints and sets the exit
//! code, so the tests never call `exit`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use cratestack_core::{ContractLock, Schema, client_contract_digest, digest_hex};

/// A report and the exit verdict: exit 0 when `ok`, 2 when `tool_error`
/// (the tool could not do its job), 1 otherwise (a failed verdict).
pub(super) struct Outcome {
    pub(super) text: String,
    pub(super) ok: bool,
    pub(super) tool_error: bool,
}

impl Outcome {
    fn verdict(text: String, ok: bool) -> Self {
        Self {
            text,
            ok,
            tool_error: false,
        }
    }

    pub(super) fn exit_code(&self) -> i32 {
        match (self.ok, self.tool_error) {
            (true, _) => 0,
            (false, true) => 2,
            (false, false) => 1,
        }
    }
}

/// `check --json` for a tool error: the same document shape, `ok: false`
/// and an `error`, so a CI parser never meets an empty stdout.
pub(super) fn json_tool_error(error: &anyhow::Error, client: Option<String>) -> Outcome {
    let doc = serde_json::json!({
        "ok": false, "error": format!("{error:#}"), "client_contract": client,
        "locked": false, "incompatible": [],
    });
    Outcome {
        text: format!("{doc:#}\n"),
        ok: false,
        tool_error: true,
    }
}

pub(super) enum Prune {
    Op(String),
    Before(String),
    Keep(usize),
    Generation(String),
}

fn read(path: &Path) -> Result<ContractLock> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the contract lock {}", path.display()))?;
    ContractLock::parse(&text).with_context(|| format!("{}", path.display()))
}

fn write(path: &Path, lock: &ContractLock) -> Result<()> {
    std::fs::write(path, lock.to_json())
        .with_context(|| format!("cannot write the contract lock {}", path.display()))
}

fn short(hex: &str) -> &str {
    &hex[..hex.len().min(12)]
}

/// Record the current contracts. A missing file starts a new lock; an
/// existing one that the current contract breaks is left untouched (exit 1):
/// pruning the broken ops first is the deliberate step.
pub(super) fn lock(schema: &Schema, path: &Path, note: &str, date: &str) -> Result<Outcome> {
    let mut lock = if path.exists() {
        read(path)?
    } else {
        ContractLock::new()
    };
    let broken = lock.incompatible(schema);
    if !broken.is_empty() {
        let mut text = String::from("not locked: the current contract breaks locked ones\n");
        broken
            .iter()
            .for_each(|b| text.push_str(&format!("  {b}\n")));
        return Ok(Outcome::verdict(text, false));
    }
    let client = digest_hex(&client_contract_digest(schema));
    let text = if lock.lock_generation(schema, date, note)? {
        write(path, &lock)?;
        format!(
            "locked generation {} ({} generation{} in {})\n",
            short(&client),
            lock.generations.len(),
            if lock.generations.len() == 1 { "" } else { "s" },
            path.display()
        )
    } else {
        format!("already locked: {}\n", short(&client))
    };
    Ok(Outcome::verdict(text, true))
}

/// The CI gate: fails when the current contract is not a locked generation
/// or breaks a locked one.
pub(super) fn check(schema: &Schema, path: &Path, json: bool) -> Result<Outcome> {
    let client = digest_hex(&client_contract_digest(schema));
    let lock = match read(path) {
        Ok(lock) => lock,
        Err(error) if json => return Ok(json_tool_error(&error, Some(client))),
        Err(error) => return Err(error),
    };
    let locked = lock.is_locked(schema);
    let broken = lock.incompatible(schema);
    let ok = locked && broken.is_empty();
    if json {
        let broken: Vec<_> = broken
            .iter()
            .map(|b| serde_json::json!({ "op": b.op, "digest": b.digest, "reasons": b.reasons }))
            .collect();
        let doc = serde_json::json!({
            "ok": ok, "client_contract": client, "locked": locked, "incompatible": broken,
        });
        let text = format!("{}\n", serde_json::to_string_pretty(&doc)?);
        return Ok(Outcome::verdict(text, ok));
    }
    let mut text = String::new();
    if !locked {
        text.push_str(&format!(
            "the current contract {} is not locked; run `cratestack contract lock` \
             before shipping a client built from it\n",
            short(&client)
        ));
    }
    broken.iter().for_each(|b| text.push_str(&format!("{b}\n")));
    if ok {
        text = format!(
            "contract lock OK: {} is locked, {} generation{} compatible\n",
            short(&client),
            lock.generations.len(),
            if lock.generations.len() == 1 { "" } else { "s" }
        );
    }
    Ok(Outcome::verdict(text, ok))
}

pub(super) fn prune(path: &Path, what: Prune) -> Result<Outcome> {
    let mut lock = read(path)?;
    let pruned = match what {
        Prune::Op(key) => {
            let pruned = lock.prune_op(&key);
            if pruned.entries == 0 {
                bail!("no generation in {} carries the op `{key}`", path.display());
            }
            pruned
        }
        Prune::Before(date) => lock.prune_before(&date)?,
        Prune::Keep(keep) => lock.prune_keep(keep),
        Prune::Generation(prefix) => lock.prune_generation(&prefix)?,
    };
    write(path, &lock)?;
    let text = format!(
        "pruned {} generation(s), {} entr{}, {} stored contract(s)\n",
        pruned.generations,
        pruned.entries,
        if pruned.entries == 1 { "y" } else { "ies" },
        pruned.contracts
    );
    Ok(Outcome::verdict(text, true))
}
