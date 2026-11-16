//! The frames table: which subscriptions a schema marks `@frames(unsigned)`
//! (cratestack#1378), keyed by the op key the contract tables use.

use crate::schema::{FramesMode, Schema};

use super::ops::{OpTarget, client_ops};

/// Every marked subscription of `schema`, as `(op key, mode)` sorted by key
/// (what the macros emit as `SUBSCRIPTION_FRAMES`). A subscription the schema
/// does not mark has no row, and a model feed has none either: `@@subscribe`
/// takes no frames mode.
pub fn subscription_frames(schema: &Schema) -> Vec<(String, FramesMode)> {
    client_ops(schema)
        .into_iter()
        .filter_map(|op| match op.target {
            OpTarget::SubscriptionProcedure(procedure) => {
                procedure.frames().map(|mode| (op.key, mode))
            }
            _ => None,
        })
        .collect()
}
