/// Result of a `.select(...)`-projected read. Holds the model with
/// only the selected columns populated — non-selected fields carry
/// their type's `Default::default()` value (`""` for `String`, `0`
/// for integers, `None` for `Option<T>`, etc.).
///
/// **Caller responsibility:** check [`Self::is_selected`] before
/// reading a field if you need to distinguish "real zero-valued DB
/// row" from "the runtime didn't fetch this column". For typical use
/// — fetch one or two specific columns for a route that needs only
/// those — just read the fields you asked for and don't read the
/// others.
///
/// **Compile-time constraint:** every model field type must impl
/// `Default`. The codegen emits a `#[derive(Default)]` on the model
/// struct; any field type that doesn't satisfy `Default` (typically a
/// `Json<MyCustomType>` where `MyCustomType` doesn't derive Default)
/// becomes a compile error at the `include_server_schema!` /
/// `include_embedded_schema!` boundary. Wrap the offending field in
/// `Option` or derive `Default` on the custom struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection<T> {
    pub value: T,
    pub selected: Vec<&'static str>,
}

impl<T> Projection<T> {
    /// Consume the projection and return the underlying model. The
    /// selection metadata is dropped — only do this when you already
    /// know which fields you asked for.
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Was this SQL column populated by the runtime? Pass the column's
    /// SQL name (the `sql_name` from `ModelColumn`, typically the
    /// snake_case form). Reading the corresponding Rust field is
    /// only meaningful when this returns `true`.
    pub fn is_selected(&self, column: &str) -> bool {
        self.selected.contains(&column)
    }
}
