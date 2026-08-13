//! Hook-name derivation for the `swr` preset's per-operation hooks
//! (issue #305). Mirrors `crate::naming::model_fn_names` one level up:
//! same verb, but a *read* operation's hook drops it (`listUsers` ->
//! `useUsers`, `getUser` -> `useUser` — issue #305's own story
//! statement's example) while a *write* operation's hook keeps it
//! (`createUser` -> `useCreateUser`), so a hook's name alone tells you
//! whether it's a `useSWR` read or a `useSWRMutation` write. Split into
//! its own file (rather than growing `crate::naming`, already at this
//! repo's ~200-LoC convention from #304) per that convention.

use crate::naming::{pluralize, to_pascal_case};

/// The five hook names a model gets under the `swr` preset — computed
/// once per model (`crate::swr::context`) and shared between the
/// per-model file context (which renders the hook bodies) and the model
/// summary (README/index, which only need the names).
pub(crate) struct ModelHookNames {
    pub(crate) list: String,
    pub(crate) get: String,
    pub(crate) create: String,
    pub(crate) update: String,
    pub(crate) delete: String,
}

pub(crate) fn model_hook_names(model_name: &str) -> ModelHookNames {
    let pascal = to_pascal_case(model_name);
    ModelHookNames {
        list: format!("use{}", pluralize(&pascal)),
        get: format!("use{pascal}"),
        create: format!("useCreate{pascal}"),
        update: format!("useUpdate{pascal}"),
        delete: format!("useDelete{pascal}"),
    }
}
