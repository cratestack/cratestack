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

use cratestack_core::{Field, MCP_MAX_PAGE_SIZE, Model, Schema, TypeArity, model_internal_actions};

use crate::shared::is_primary_key;

/// One `@@mcp(resource: ...)` model.
pub(in crate::include) struct ResourcePlan {
    pub(in crate::include) model: Model,
    pub(in crate::include) segment: String,
    /// `max_page_size:`, or the framework maximum (Q3).
    pub(in crate::include) max_page_size: u32,
    /// `<name>` in `cratestack://<name>/<segment>`: the `mcp { name = "..." }`
    /// value, the same for every resource of the schema.
    pub(in crate::include) authority: String,
    pub(in crate::include) primary_key: Field,
}

/// Primary-key types an id in a URI can be parsed into, with the same
/// `FromStr` text a REST path segment uses.
const ADDRESSABLE_KEYS: [&str; 4] = ["String", "Cuid", "Int", "Uuid"];

pub(super) fn resource_plans(schema: &Schema) -> Result<Vec<ResourcePlan>, String> {
    let exposed: Vec<&Model> = schema.models.iter().filter(|m| m.mcp.is_some()).collect();
    if exposed.is_empty() {
        return Ok(Vec::new());
    }
    let authority = authority(schema)?;
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

/// The `mcp { name = "..." }` value: `name = "blog"` serves
/// `cratestack://blog/...` (maintainer decision on cratestack#1040). The
/// parser has already required it whenever a model is a resource, and
/// checked it is `[a-z0-9-]+`, so the authority needs no percent-encoding
/// and cannot read as a port or userinfo.
///
/// There is deliberately no fallback to the `.cstack` file's name, which
/// phase 5 first used: a URI an agent holds must not change when the file
/// is renamed or moved, and two schemas whose files share a name must not
/// serve the same URIs. A missing name is therefore an error here too,
/// should a schema ever reach the macro without the parser's check.
fn authority(schema: &Schema) -> Result<String, String> {
    schema
        .mcp
        .as_ref()
        .and_then(|config| config.name.as_ref())
        .map(|name| name.value.clone())
        .ok_or_else(|| {
            "MCP resources are addressed as `cratestack://<name>/<segment>`, and the schema's \
             `mcp { }` block has no `name = \"...\"`: add one, for example `name = \"blog\"`"
                .to_owned()
        })
}
