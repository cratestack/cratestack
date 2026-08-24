//! Create/get/delete handler tokens. Each delegates to the per-model
//! `Cratestack` accessor after transport / auth / decode preflight.
//! Split per-verb (plus a shared `response_tail` helper) to stay under
//! the repo's 200-LoC-per-file convention — `update` lives in the
//! sibling `handlers_update.rs` module instead, since its `@version`
//! ETag flow doesn't share enough with these three to be worth forcing
//! into the same directory.

mod create;
mod delete;
mod get;
mod response_tail;

pub(super) use create::build_create_handler;
pub(super) use delete::build_delete_handler;
pub(super) use get::build_get_handler;
