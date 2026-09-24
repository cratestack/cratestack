//! The resource half of the server plan (cratestack#1040): which models
//! are served as MCP resources, under which URI authority, and the
//! declarations that cannot be served faithfully and are refused.
//!
//! Each refusal is a hard error rather than a silently narrower surface,
//! for ADR 0002 § Validation's reason: MCP exposure decides what an agent
//! can reach, so an annotation must never parse and then quietly serve
//! less (or more) than it says. None of them re-decides an ADR question;
//! each names a case the ADR does not cover, and the report on #1040 lists
//! them for the maintainer.

use std::path::Path;

use cratestack_core::{Field, MCP_MAX_PAGE_SIZE, Model, Schema, TypeArity, model_internal_actions};

use crate::shared::is_primary_key;

/// One `@@mcp(resource: ...)` model.
pub(in crate::include) struct ResourcePlan {
    pub(in crate::include) model: Model,
    pub(in crate::include) segment: String,
    /// `max_page_size:`, or the framework maximum (Q3).
    pub(in crate::include) max_page_size: u32,
    /// `<schema>` in `cratestack://<schema>/<segment>`.
    pub(in crate::include) authority: String,
    pub(in crate::include) primary_key: Field,
}

/// Primary-key types an id in a URI can be parsed into, with the same
/// `FromStr` text a REST path segment uses.
const ADDRESSABLE_KEYS: [&str; 4] = ["String", "Cuid", "Int", "Uuid"];

pub(super) fn resource_plans(
    schema: &Schema,
    schema_file: &str,
) -> Result<Vec<ResourcePlan>, String> {
    let exposed: Vec<&Model> = schema.models.iter().filter(|m| m.mcp.is_some()).collect();
    if exposed.is_empty() {
        return Ok(Vec::new());
    }
    let authority = authority(schema_file)?;
    exposed
        .into_iter()
        .map(|model| plan(model, &authority))
        .collect()
}

fn plan(model: &Model, authority: &str) -> Result<ResourcePlan, String> {
    let exposure = model.mcp.as_ref().expect("filtered on `mcp`");
    let internal = model_internal_actions(model);
    if let Some(verb) = ["get", "list"]
        .into_iter()
        .find(|verb| internal.contains(*verb))
    {
        return Err(format!(
            "`@@mcp(resource: \"{}\")` on model `{}` contradicts its `@@internal(...)`, which \
             keeps the model's `{verb}` off the wire: a resource would put it back on. Remove \
             one of the two.",
            exposure.resource, model.name
        ));
    }
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| {
            format!(
                "`@@mcp(resource: ...)` on model `{}` needs a single `@id` field to address \
                 a record by URI",
                model.name
            )
        })?;
    let addressable = matches!(primary_key.ty.arity, TypeArity::Required)
        && ADDRESSABLE_KEYS.contains(&primary_key.ty.name.as_str());
    if !addressable {
        return Err(format!(
            "`@@mcp(resource: ...)` on model `{}` cannot address a record by URI: its `@id` \
             field `{}` is `{}`, and a resource id must be one of {}.",
            model.name,
            primary_key.name,
            primary_key.ty.name,
            ADDRESSABLE_KEYS.join(", ")
        ));
    }
    Ok(ResourcePlan {
        model: model.clone(),
        segment: exposure.resource.clone(),
        max_page_size: exposure.max_page_size.unwrap_or(MCP_MAX_PAGE_SIZE),
        authority: authority.to_owned(),
        primary_key: primary_key.clone(),
    })
}

/// The schema file's name without its extension: `blog.cstack` serves
/// `cratestack://blog/...`. Only RFC 3986 unreserved characters, so the
/// authority never needs percent-encoding and never looks like a port or
/// userinfo. The IR has no schema name, and choosing one is a maintainer
/// question (#1040); the file stem is the placeholder that needs no syntax.
fn authority(schema_file: &str) -> Result<String, String> {
    let stem = Path::new(schema_file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let unreserved = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    if stem.is_empty() || !stem.chars().all(unreserved) {
        return Err(format!(
            "MCP resources are addressed as `cratestack://<schema>/<segment>`, where `<schema>` \
             is the schema file's name without `.cstack`; `{schema_file}` gives `{stem}`, which \
             is not made of letters, digits, `-`, `.`, `_` and `~` only. Rename the file."
        ));
    }
    Ok(stem.to_owned())
}
