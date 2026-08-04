//! Public `Schema → IR` projection seam.
//!
//! [`project`] lowers a parsed `.cstack` [`Schema`] into [`Projections`]
//! — the backend-agnostic table/view shape [`crate::diff_projections`]
//! compares. `Schema` carries source-level constructs a live-database
//! introspector can never recover (`mixins`, `procedures`, authored
//! view SQL, `auth`, attribute provenance, …), so this seam exists to
//! give a future introspector (Phase B, issue #204) a second, lower-
//! level way to produce the same IR shape without ever holding a
//! `Schema` — see `docs/design/migrate-baseline.md` §5.1.
//!
//! Note: enum *types* are not a separate field on [`Projections`].
//! The diff engine never compares them as a standalone entity today —
//! [`crate::convert::project_model`] resolves a `.cstack` enum
//! declaration into a `CHECK` constraint on the owning column at
//! projection time, so an enum's effect on the diff is already
//! captured inside the owning table's `TableProjection::checks`. A
//! future introspector reconstructs the equivalent CHECK from
//! `pg_enum`/`pg_type` rather than needing a separate enum bucket
//! here.

use std::collections::BTreeMap;

use cratestack_core::Schema;
use serde::{Deserialize, Serialize};

use crate::convert::{TableProjection, project_model};
use crate::diff::views::{ViewProjection, project_views};

/// Backend-agnostic snapshot of a schema's SQL shape: every table
/// (columns, indexes, checks, foreign keys) and every view, keyed by
/// their SQL name. The unit [`crate::diff_projections`] compares.
///
/// `Serialize`/`Deserialize` (issue #205): this is the value
/// [`crate::Snapshot`] persists to `schema.snapshot.json` — both for
/// the ordinary `.cstack`-authored path (`project()`, going through
/// `cratestack migrate diff`) and for `cratestack migrate baseline`,
/// which has no `Schema` on the "previous state" side at all, only
/// whatever [`crate::introspect::postgres::introspect`] produced.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Projections {
    pub tables: BTreeMap<String, TableProjection>,
    pub views: BTreeMap<String, ViewProjection>,
}

/// Project a parsed `.cstack` [`Schema`] into its [`Projections`] IR
/// shape.
pub fn project(schema: &Schema) -> Projections {
    Projections {
        tables: project_tables(schema),
        views: project_views(schema),
    }
}

fn project_tables(schema: &Schema) -> BTreeMap<String, TableProjection> {
    schema
        .models
        .iter()
        .map(|model| {
            let projection = project_model(model, schema);
            (projection.name.clone(), projection)
        })
        .collect()
}
