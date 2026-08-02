//! [`build_model_summary`] — split out of `context.rs` to keep it under
//! this repo's ~200-LoC convention (issue #305 grew it past that
//! threshold by wiring in hook names alongside the existing function
//! names). One self-contained function: turns a `Model` into the flat
//! [`SwrModelSummary`] the shared context (`README.md`/`index.ts`/
//! `src/swr-keys.ts`) iterates over.

use cratestack_core::Model;

use crate::naming::{model_fn_names, to_kebab_case};
use crate::views::build_model_api;

use super::hook_naming::model_hook_names;
use super::views::SwrModelSummary;

pub(super) fn build_model_summary(model: &Model) -> SwrModelSummary {
    let api = build_model_api(model);
    let fns = model_fn_names(&model.name);
    let hooks = model_hook_names(&model.name);
    SwrModelSummary {
        name: model.name.clone(),
        file_stem: to_kebab_case(&model.name),
        accessor: api.accessor,
        route: api.route,
        primary_key_type: api.primary_key_type,
        allows_create: api.allows_create,
        list_fn: fns.list,
        get_fn: fns.get,
        create_fn: fns.create,
        update_fn: fns.update,
        delete_fn: fns.delete,
        list_hook: hooks.list,
        get_hook: hooks.get,
        create_hook: hooks.create,
        update_hook: hooks.update,
        delete_hook: hooks.delete,
    }
}
