#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    Contains,
    StartsWith,
    IsNull,
    IsNotNull,
    /// `(col IS NULL OR col = $1)` — for the "nullable column matches
    /// either the bound value or null" pattern that's otherwise
    /// awkward to express via `Any([is_null, eq])` (the latter
    /// double-binds the value when the same caller wants the null-
    /// punning behavior elsewhere). Single-bind, single op.
    EqOrNull,
}
