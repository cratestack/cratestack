//! Type-level markers for the generated typestate builders.
//!
//! Every struct-shaped type `include_*_schema!` emits — model structs,
//! `Create{Model}Input`/`Update{Model}Input`, `{Model}Where`,
//! `{Model}OrderByClause`, `{Model}FindManyInput`, `view` structs,
//! `type` structs, and per-procedure `Args` — gets a companion
//! `{Type}Builder` whose `build()` method only *exists* once every
//! required field has been set. The "has this slot been filled" bit is
//! carried in the type system by these two markers, one type parameter
//! per required field:
//!
//! ```text
//! pub struct CreateBoardInputBuilder<S0 = Unset, S1 = Unset> { .. }
//!
//! impl<S0, S1> CreateBoardInputBuilder<S0, S1> {
//!     pub fn id(self, value: i64) -> CreateBoardInputBuilder<Set, S1> { .. }
//!     pub fn name(self, value: impl Into<String>) -> CreateBoardInputBuilder<S0, Set> { .. }
//! }
//!
//! impl CreateBoardInputBuilder<Set, Set> {
//!     pub fn build(self) -> CreateBoardInput { .. }   // infallible
//! }
//! ```
//!
//! Forgetting `.name(..)` is therefore a *compile* error ("no method
//! named `build` found for struct `CreateBoardInputBuilder<Set, Unset>`"),
//! not a runtime `Result` the caller has to handle. Optional fields
//! (`Option<T>` and `Vec<T>` — the two shapes whose `Default` is exactly
//! the right "caller said nothing" value) get no state parameter at all:
//! their setters return `Self`, so an all-optional struct like
//! `{Model}Where` has a plain non-generic builder.
//!
//! Both markers are uninhabited on purpose: they exist only as type-level
//! bits inside a `PhantomData`, and nothing should ever hold a value of
//! either one.

/// Type-level "this required field has been set".
pub enum Set {}

/// Type-level "this required field has not been set yet" — the default
/// every state parameter starts at.
pub enum Unset {}
